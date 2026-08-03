//! Sun and moon for an entry's place and date, computed locally — no network.
//!
//! Sunrise/sunset are a function of coordinates and date (the [`sunrise`] crate);
//! the moon phase is a function of the date alone (a synodic-month formula). So
//! celestial data can be filled in instantly and offline the moment a location is
//! set, independent of the (networked) weather fetch.

use chrono::{DateTime, Utc};
use jiff::Zoned;
use notema_domain::{Celestial, Coordinates};
use sunrise::{Coordinates as SolarCoordinates, SolarDay, SolarEvent};

/// The mean length of a synodic month (new moon to new moon), in days.
const SYNODIC_MONTH_DAYS: f64 = 29.530_588_853;
/// A reference new moon: 2000-01-06T18:14:00Z, as a Unix timestamp (seconds).
const REFERENCE_NEW_MOON_UNIX: f64 = 947_182_440.0;

/// Compute the `[celestial]` table for a point and instant. Sunrise/sunset use
/// the location's date and are rendered at the entry's own UTC offset;
/// `moon_phase`/`moon_phase_name` come from the instant. Sunrise/sunset are left
/// `None` when the sun never crosses the horizon that day (polar day/night) or
/// the coordinates are out of range.
pub fn compute_celestial(coordinates: Coordinates, datetime: Zoned) -> Celestial {
    let lat = coordinates.latitude();
    let lon = coordinates.longitude();
    // `sunrise` is chrono-typed: feed it a `chrono::NaiveDate` and read back
    // `chrono::DateTime<Utc>`, converting to jiff at the boundary. This bridge is
    // why `notema-context` keeps a minimal direct `chrono` dependency.
    let (sunrise_utc, sunset_utc) = match (SolarCoordinates::new(lat, lon), solar_date(&datetime)) {
        (Some(coord), Some(date)) => {
            let day = SolarDay::new(coord, date);
            (
                day.event_time(SolarEvent::Sunrise),
                day.event_time(SolarEvent::Sunset),
            )
        }
        _ => (None, None),
    };
    // Daylight duration: only when both events occur (not polar day/night).
    let day_length_seconds = match (sunrise_utc, sunset_utc) {
        (Some(rise), Some(set)) => {
            u64::try_from(set.signed_duration_since(rise).num_seconds()).ok()
        }
        _ => None,
    };
    // Render each event at the entry's own UTC offset, matching its wall clock.
    let offset = datetime.offset();
    let render = |event: Option<DateTime<Utc>>| {
        event.and_then(|utc| {
            jiff::Timestamp::from_second(utc.timestamp())
                .ok()
                .map(|instant| instant.display_with_offset(offset).to_string())
        })
    };

    let phase = moon_phase_fraction(datetime.timestamp().as_second());
    Celestial {
        moon_phase: Some(phase),
        moon_phase_name: Some(moon_phase_name(phase).to_string()),
        sunrise: render(sunrise_utc),
        sunset: render(sunset_utc),
        day_length_seconds,
    }
}

/// The entry's civil date at its own offset, as the `chrono::NaiveDate` the
/// `sunrise` crate expects. `None` only if the calendar date is somehow out of
/// chrono's range, which a real entry never hits.
fn solar_date(datetime: &Zoned) -> Option<chrono::NaiveDate> {
    let date = datetime.date();
    chrono::NaiveDate::from_ymd_opt(
        i32::from(date.year()),
        u32::from(date.month().unsigned_abs()),
        u32::from(date.day().unsigned_abs()),
    )
}

/// The moon's position in its cycle as a fraction in `[0, 1)`: 0 is new, 0.5 is
/// full. Derived from the time elapsed since a known new moon, folded onto one
/// synodic month.
fn moon_phase_fraction(unix_seconds: i64) -> f64 {
    let elapsed_days = (unix_seconds as f64 - REFERENCE_NEW_MOON_UNIX) / 86_400.0;
    (elapsed_days / SYNODIC_MONTH_DAYS).rem_euclid(1.0)
}

/// The named phase for a `[0, 1)` cycle fraction, split into the eight
/// conventional phases centered on new / quarters / full.
fn moon_phase_name(fraction: f64) -> &'static str {
    match (fraction * 8.0).round() as i64 % 8 {
        0 => "new",
        1 => "waxing-crescent",
        2 => "first-quarter",
        3 => "waxing-gibbous",
        4 => "full",
        5 => "waning-gibbous",
        6 => "last-quarter",
        _ => "waning-crescent",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(y: i16, m: i8, d: i8) -> Zoned {
        jiff::civil::date(y, m, d)
            .at(12, 0, 0, 0)
            .to_zoned(jiff::tz::TimeZone::UTC)
            .unwrap()
    }

    #[test]
    fn sunrise_before_sunset_at_mid_latitude() {
        // Berlin in summer: both events occur and sunrise precedes sunset.
        let celestial = compute_celestial(
            Coordinates::try_new(52.52, 13.405).unwrap(),
            utc(2026, 7, 1),
        );
        let sunrise = celestial.sunrise.expect("summer sunrise exists");
        let sunset = celestial.sunset.expect("summer sunset exists");
        assert!(sunrise < sunset, "{sunrise} !< {sunset}");
        // Early-July Berlin has a long day — roughly 16½ hours of daylight.
        let hours = celestial.day_length_seconds.expect("day length exists") as f64 / 3600.0;
        assert!((16.0..17.0).contains(&hours), "day length was {hours}h");
    }

    #[test]
    fn polar_day_has_no_sunrise_no_day_length_but_still_a_moon_phase() {
        // Above the Arctic Circle at midsummer the sun never sets.
        let celestial =
            compute_celestial(Coordinates::try_new(80.0, 20.0).unwrap(), utc(2026, 6, 21));
        assert!(celestial.sunrise.is_none());
        assert!(celestial.sunset.is_none());
        assert!(celestial.day_length_seconds.is_none());
        assert!(celestial.moon_phase.is_some());
    }

    #[test]
    fn moon_phase_name_buckets_cover_the_cycle() {
        assert_eq!(moon_phase_name(0.0), "new");
        assert_eq!(moon_phase_name(0.25), "first-quarter");
        assert_eq!(moon_phase_name(0.5), "full");
        assert_eq!(moon_phase_name(0.75), "last-quarter");
        // Just shy of a full cycle rounds back to new.
        assert_eq!(moon_phase_name(0.99), "new");
    }

    #[test]
    fn moon_phase_fraction_is_near_full_at_a_known_full_moon() {
        // 2026-01-03 was a full moon; the fraction should sit near 0.5.
        let instant = jiff::civil::date(2026, 1, 3)
            .at(10, 0, 0, 0)
            .to_zoned(jiff::tz::TimeZone::UTC)
            .unwrap();
        let fraction = moon_phase_fraction(instant.timestamp().as_second());
        assert!((fraction - 0.5).abs() < 0.05, "fraction was {fraction}");
    }
}
