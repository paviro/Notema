use std::path::Path;

use jiff::ToSpan;
use jiff::civil::Date;

use crate::Entry;

/// The date an entry is grouped under: its creation timestamp when present,
/// otherwise the date encoded in its filename.
pub fn entry_group_date(entry: &Entry) -> Option<Date> {
    entry
        .created_time()
        .map(|timestamp| timestamp.date())
        .or_else(|| entry_date_from_path(&entry.path))
}

/// Parse the leading `YYYY-MM-DD` of an entry filename stem into a date.
pub fn entry_date_from_path(path: &Path) -> Option<Date> {
    let stem = path.file_stem()?.to_str()?;
    let date = stem.get(..10)?;
    Date::strptime("%Y-%m-%d", date).ok()
}

/// How a [`DateSpec`] is compared against an entry's date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateBound {
    /// Inside the spec's range.
    On,
    /// Strictly earlier than the range.
    Before,
    /// Strictly later than the range.
    After,
}

/// The unit of a relative date like `7d` or `3m`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateUnit {
    Days,
    Weeks,
    Months,
    Years,
}

/// A year-month-day pattern where any component may be left open with `*`, and
/// trailing components may be omitted entirely. `2026-07` and `2026-07-*` mean
/// the same thing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DatePattern {
    pub year: Option<i32>,
    pub month: Option<u32>,
    pub day: Option<u32>,
}

/// A point or span of time named by a search query. Dates are written
/// year-first, so adding a component narrows the span without moving the ones
/// before it: `2026`, `2026-07`, `2026-07-25`. `*` leaves a component open, so
/// `*-07-25` is every 25 July.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateSpec {
    Pattern(DatePattern),
    /// The single day `count` units before today.
    Relative {
        count: u32,
        unit: DateUnit,
    },
    Today,
    Yesterday,
}

/// A parsed `date:`/`before:`/`after:` search filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateFilter {
    pub bound: DateBound,
    pub spec: DateSpec,
}

impl DatePattern {
    fn matches(&self, date: Date) -> bool {
        self.year.is_none_or(|year| i32::from(date.year()) == year)
            && self.month.is_none_or(|month| u32::from(date.month().unsigned_abs()) == month)
            && self.day.is_none_or(|day| u32::from(date.day().unsigned_abs()) == day)
    }

    /// The inclusive span this pattern covers, or `None` when it has an open
    /// component and so recurs rather than naming one stretch of time.
    fn range(&self) -> Option<(Date, Date)> {
        let year = self.year?;
        let Some(month) = self.month else {
            // An open month with a fixed day (`2026-*-25`) recurs monthly.
            return self.day.is_none().then(|| {
                (
                    ymd(year, 1, 1).unwrap(),
                    ymd(year, 12, 31).unwrap(),
                )
            });
        };
        let start = ymd(year, month, 1)?;
        match self.day {
            Some(day) => {
                let date = ymd(year, month, day)?;
                Some((date, date))
            }
            None => Some((start, start.checked_add(1.months()).ok()?.yesterday().ok()?)),
        }
    }
}

/// Build a civil [`Date`] from the pattern's wider integer components, narrowing
/// to jiff's `i16`/`i8` and returning `None` when the values fall outside a real
/// calendar date.
fn ymd(year: i32, month: u32, day: u32) -> Option<Date> {
    let year = i16::try_from(year).ok()?;
    let month = i8::try_from(month).ok()?;
    let day = i8::try_from(day).ok()?;
    Date::new(year, month, day).ok()
}

impl DateSpec {
    /// Parse the value of a date search filter, or `None` when it isn't one.
    ///
    /// Parsing takes the longest valid prefix, so a half-typed `2026-` or
    /// `2026-0` still resolves to `2026` instead of blanking the results while
    /// the user is mid-token.
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        match value.to_ascii_lowercase().as_str() {
            "today" => return Some(Self::Today),
            "yesterday" => return Some(Self::Yesterday),
            _ => {}
        }
        if let Some(relative) = parse_relative(value) {
            return Some(relative);
        }

        let mut parts = value.split('-');
        let year = parse_component(parts.next()?, parse_year)?;
        let mut pattern = DatePattern {
            year: year.map(|year| year as i32),
            ..DatePattern::default()
        };
        let Some(month) = parts
            .next()
            .and_then(|raw| parse_component(raw, parse_month))
        else {
            return Some(Self::Pattern(pattern));
        };
        pattern.month = month;
        if let Some(day) = parts.next().and_then(|raw| parse_component(raw, parse_day)) {
            // A day is only kept when it can exist in the named month.
            let fits = match (pattern.year, pattern.month, day) {
                (_, Some(month), Some(day)) => day <= days_in_month(pattern.year, month),
                _ => true,
            };
            if fits {
                pattern.day = day;
            }
        }
        Some(Self::Pattern(pattern))
    }

    /// The inclusive range this spec covers, or `None` when it recurs and so has
    /// no single range.
    pub fn range(&self, today: Date) -> Option<(Date, Date)> {
        match *self {
            Self::Pattern(pattern) => pattern.range(),
            Self::Relative { count, unit } => {
                let date = shift_back(today, count, unit)?;
                Some((date, date))
            }
            Self::Today => Some((today, today)),
            Self::Yesterday => {
                let date = today.yesterday().ok()?;
                Some((date, date))
            }
        }
    }
}

impl DateFilter {
    /// Whether `date` satisfies this filter. `today` is passed in rather than
    /// read from the clock so the comparison stays pure and testable.
    pub fn matches(&self, date: Date, today: Date) -> bool {
        // A pattern compares component-wise, which is what lets an open
        // component match across years or months.
        if let (DateBound::On, DateSpec::Pattern(pattern)) = (self.bound, self.spec) {
            return pattern.matches(date);
        }
        let Some((start, end)) = self.spec.range(today) else {
            // A recurring pattern has no range, so there is no side to be on.
            return false;
        };
        match self.bound {
            DateBound::On => date >= start && date <= end,
            DateBound::Before => date < start,
            DateBound::After => date > end,
        }
    }
}

/// Parse one `YYYY`/`MM`/`DD` slot: `*` leaves it open, a valid number fixes it,
/// and anything else (including the empty string of a trailing `-`) ends the
/// pattern.
fn parse_component(raw: &str, parse: impl Fn(&str) -> Option<u32>) -> Option<Option<u32>> {
    if raw == "*" {
        return Some(None);
    }
    parse(raw).map(Some)
}

fn parse_year(raw: &str) -> Option<u32> {
    (raw.len() == 4 && raw.bytes().all(|byte| byte.is_ascii_digit())).then(|| raw.parse().ok())?
}

fn parse_month(raw: &str) -> Option<u32> {
    parse_two_digit(raw).filter(|month| (1..=12).contains(month))
}

fn parse_day(raw: &str) -> Option<u32> {
    parse_two_digit(raw).filter(|day| (1..=31).contains(day))
}

fn parse_two_digit(raw: &str) -> Option<u32> {
    ((1..=2).contains(&raw.len()) && raw.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| raw.parse().ok())?
}

/// Days in `month`, using a leap year when the year is open so `*-02-29` holds.
fn days_in_month(year: Option<i32>, month: u32) -> u32 {
    match ymd(year.unwrap_or(2024), month, 1) {
        Some(start) => u32::from(start.days_in_month().unsigned_abs()),
        None => 0,
    }
}

fn parse_relative(raw: &str) -> Option<DateSpec> {
    // Take the unit off with next_back, not split_at: a byte index one short of
    // the end is no char boundary when the last char is multibyte.
    let mut chars = raw.chars();
    let unit = match chars.next_back()? {
        'd' | 'D' => DateUnit::Days,
        'w' | 'W' => DateUnit::Weeks,
        'm' | 'M' => DateUnit::Months,
        'y' | 'Y' => DateUnit::Years,
        _ => return None,
    };
    let count: u32 = chars.as_str().parse().ok()?;
    Some(DateSpec::Relative { count, unit })
}

fn shift_back(today: Date, count: u32, unit: DateUnit) -> Option<Date> {
    let count = i64::from(count);
    let span = match unit {
        DateUnit::Days => count.days(),
        DateUnit::Weeks => count.weeks(),
        DateUnit::Months => count.months(),
        DateUnit::Years => count.years(),
    };
    today.checked_sub(span).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EntryEncryptionState, Timestamp};
    use std::path::PathBuf;

    fn date(year: i16, month: i8, day: i8) -> Date {
        jiff::civil::date(year, month, day)
    }

    fn entry(created_at: Option<&str>, path: &str) -> Entry {
        Entry {
            id: "id".to_string(),
            journal: "work".to_string(),
            path: PathBuf::from(path),
            encryption_state: EntryEncryptionState::Plain,
            created_at: created_at.map(Timestamp::parse),
            edited_at: None,
            preview: String::new(),
            activities: Vec::new(),
            feelings: Vec::new(),
            people: Vec::new(),
            tags: Vec::new(),
            mood: None,
            starred: false,
            location: None,
            weather: None,
            celestial: None,
            air_quality: None,
            import: None,
            body: String::new(),
            word_count: 0,
            search_haystack: String::new(),
            warning: None,
        }
    }

    #[test]
    fn group_date_prefers_created_timestamp() {
        let entry = entry(Some("2026-07-01T10:23:00+02:00"), "work/2026-01-01/id.md");

        assert_eq!(entry_group_date(&entry), Some(jiff::civil::date(2026, 7, 1)));
    }

    #[test]
    fn group_date_falls_back_to_filename_date() {
        let entry = entry(None, "work/2026/07/01/2026-07-01T10-23-00-id.md");

        assert_eq!(entry_group_date(&entry), Some(jiff::civil::date(2026, 7, 1)));
    }

    fn pattern(year: Option<i32>, month: Option<u32>, day: Option<u32>) -> Option<DateSpec> {
        Some(DateSpec::Pattern(DatePattern { year, month, day }))
    }

    /// Each component added narrows the span; `*` leaves one open.
    #[test]
    fn parses_every_spec_form() {
        assert_eq!(DateSpec::parse("2026"), pattern(Some(2026), None, None));
        assert_eq!(
            DateSpec::parse("2026-07"),
            pattern(Some(2026), Some(7), None)
        );
        assert_eq!(
            DateSpec::parse("2026-07-25"),
            pattern(Some(2026), Some(7), Some(25))
        );
        assert_eq!(DateSpec::parse("*-07-25"), pattern(None, Some(7), Some(25)));
        assert_eq!(
            DateSpec::parse("2026-*-25"),
            pattern(Some(2026), None, Some(25))
        );
        assert_eq!(DateSpec::parse("today"), Some(DateSpec::Today));
        assert_eq!(DateSpec::parse("Yesterday"), Some(DateSpec::Yesterday));
        assert_eq!(
            DateSpec::parse("7d"),
            Some(DateSpec::Relative {
                count: 7,
                unit: DateUnit::Days
            })
        );
        // Single-digit month and day are accepted alongside the padded forms.
        assert_eq!(
            DateSpec::parse("2026-7-5"),
            pattern(Some(2026), Some(7), Some(5))
        );
        // A trailing `*` is the same as omitting the component.
        assert_eq!(DateSpec::parse("2026-07-*"), DateSpec::parse("2026-07"));
    }

    /// Live search re-runs on every keystroke, so a half-typed date has to keep
    /// showing the wider result set rather than blanking until it's complete.
    #[test]
    fn partial_input_falls_back_to_the_longest_valid_prefix() {
        let year = pattern(Some(2026), None, None);
        assert_eq!(DateSpec::parse("2026-"), year);
        assert_eq!(DateSpec::parse("2026-0"), year);
        assert_eq!(
            DateSpec::parse("2026-1"),
            pattern(Some(2026), Some(1), None)
        );

        let month = pattern(Some(2026), Some(7), None);
        assert_eq!(DateSpec::parse("2026-07-"), month);
        assert_eq!(DateSpec::parse("2026-07-0"), month);
        // 32 can't be a day, so the month stands until a real one is typed.
        assert_eq!(DateSpec::parse("2026-07-32"), month);
    }

    #[test]
    fn rejects_unparseable_values() {
        for value in [
            "",
            "garbage",
            "26",
            // Day-first order is not accepted: the year (or `*`) must lead.
            "25-07-2026",
            "07-2026",
            "07-25",
            "d",
            "-7d",
            // Multibyte finals must be rejected, not split mid-char.
            "5ä",
            "7д",
            "ä",
            "٣d",
        ] {
            assert_eq!(
                DateSpec::parse(value),
                None,
                "expected {value:?} to be rejected"
            );
        }
    }

    #[test]
    fn month_and_year_ranges_cover_their_bounds() {
        let today = date(2026, 7, 25);

        assert_eq!(
            DateSpec::parse("2026-02").unwrap().range(today),
            Some((date(2026, 2, 1), date(2026, 2, 28)))
        );
        // A leap February ends on the 29th.
        assert_eq!(
            DateSpec::parse("2024-02").unwrap().range(today),
            Some((date(2024, 2, 1), date(2024, 2, 29)))
        );
        assert_eq!(
            DateSpec::parse("2026").unwrap().range(today),
            Some((date(2026, 1, 1), date(2026, 12, 31)))
        );
    }

    /// An open component makes the pattern recur, so it names no single stretch
    /// of time and `before:`/`after:` have no side to compare against.
    #[test]
    fn open_components_have_no_range() {
        let today = date(2026, 7, 25);

        assert_eq!(DateSpec::parse("*-07-25").unwrap().range(today), None);
        assert_eq!(DateSpec::parse("2026-*-25").unwrap().range(today), None);
    }

    #[test]
    fn open_month_matches_that_day_every_month() {
        let today = date(2026, 7, 25);
        let filter = DateFilter {
            bound: DateBound::On,
            spec: DateSpec::parse("2026-*-25").unwrap(),
        };

        assert!(filter.matches(date(2026, 1, 25), today));
        assert!(filter.matches(date(2026, 7, 25), today));
        assert!(!filter.matches(date(2026, 7, 24), today));
        assert!(!filter.matches(date(2025, 7, 25), today));
    }

    #[test]
    fn relative_months_clamp_across_year_ends() {
        let today = date(2026, 1, 31);

        // Three months before 31 January is 31 October, not an invalid date.
        assert_eq!(
            DateSpec::Relative {
                count: 3,
                unit: DateUnit::Months
            }
            .range(today),
            Some((date(2025, 10, 31), date(2025, 10, 31)))
        );
        // One month before 31 March clamps to the end of February.
        assert_eq!(
            DateSpec::Relative {
                count: 1,
                unit: DateUnit::Months
            }
            .range(date(2026, 3, 31)),
            Some((date(2026, 2, 28), date(2026, 2, 28)))
        );
        assert_eq!(
            DateSpec::Relative {
                count: 1,
                unit: DateUnit::Years
            }
            .range(today),
            Some((date(2025, 1, 31), date(2025, 1, 31)))
        );
        assert_eq!(
            DateSpec::Relative {
                count: 2,
                unit: DateUnit::Weeks
            }
            .range(today),
            Some((date(2026, 1, 17), date(2026, 1, 17)))
        );
    }

    #[test]
    fn anniversary_matches_that_day_in_every_year() {
        let today = date(2026, 7, 25);
        let filter = DateFilter {
            bound: DateBound::On,
            spec: DateSpec::parse("*-07-25").unwrap(),
        };

        assert!(filter.matches(date(2026, 7, 25), today));
        assert!(filter.matches(date(2019, 7, 25), today));
        assert!(!filter.matches(date(2019, 7, 24), today));
        assert!(!filter.matches(date(2019, 8, 25), today));
    }

    /// 29 February only exists in leap years, so the anniversary can't match a
    /// common year's 28th or 1 March.
    #[test]
    fn leap_day_anniversary_matches_only_real_leap_days() {
        let today = date(2026, 7, 25);
        let filter = DateFilter {
            bound: DateBound::On,
            spec: DateSpec::parse("*-02-29").unwrap(),
        };

        assert!(filter.matches(date(2024, 2, 29), today));
        assert!(!filter.matches(date(2025, 2, 28), today));
        assert!(!filter.matches(date(2025, 3, 1), today));
    }

    #[test]
    fn before_and_after_bracket_the_range() {
        let today = date(2026, 7, 25);
        let spec = DateSpec::parse("2026-07").unwrap();

        let before = DateFilter {
            bound: DateBound::Before,
            spec,
        };
        assert!(before.matches(date(2026, 6, 30), today));
        assert!(!before.matches(date(2026, 7, 1), today));

        let after = DateFilter {
            bound: DateBound::After,
            spec,
        };
        assert!(after.matches(date(2026, 8, 1), today));
        assert!(!after.matches(date(2026, 7, 31), today));

        let on = DateFilter {
            bound: DateBound::On,
            spec,
        };
        assert!(on.matches(date(2026, 7, 1), today));
        assert!(on.matches(date(2026, 7, 31), today));
        assert!(!on.matches(date(2026, 8, 1), today));
    }

    /// An open year recurs, so there is no range to sit outside of.
    #[test]
    fn before_and_after_never_match_an_open_pattern() {
        let today = date(2026, 7, 25);
        let spec = DateSpec::parse("*-07-25").unwrap();

        for bound in [DateBound::Before, DateBound::After] {
            let filter = DateFilter { bound, spec };
            assert!(!filter.matches(date(2019, 7, 25), today));
            assert!(!filter.matches(date(2019, 1, 1), today));
        }
    }

    #[test]
    fn today_and_yesterday_follow_the_supplied_date() {
        let today = date(2026, 1, 1);

        let filter = DateFilter {
            bound: DateBound::On,
            spec: DateSpec::Today,
        };
        assert!(filter.matches(date(2026, 1, 1), today));

        let filter = DateFilter {
            bound: DateBound::On,
            spec: DateSpec::Yesterday,
        };
        assert!(filter.matches(date(2025, 12, 31), today));
        assert!(!filter.matches(date(2026, 1, 1), today));
    }
}
