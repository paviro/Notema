//! Link-hint labels: the chips `o` puts beside every openable target.
//!
//! A chip is *inserted* into the body text, so labels have to be assigned while
//! the body is built — [`HintLabeller`] hands them out in document order and the
//! paragraph wraps around them.

use ratatui::style::{Modifier, Style};

use crate::tui::{app::ReaderLinkTarget, theme::Theme};

/// Label letters, home row first so the earliest targets take the easiest keys.
/// The whole alphabet is available because the mode claims every letter while it
/// is up, and a wide alphabet keeps labels one character wide for all but the
/// densest entries.
const ALPHABET: [char; 26] = [
    'a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l', 'q', 'w', 'e', 'r', 't', 'y', 'u', 'i', 'o', 'p',
    'z', 'x', 'c', 'v', 'b', 'n', 'm',
];

/// `count` distinct labels, all of the shortest length that fits: one character
/// up to 26 targets, two up to 676.
///
/// Uniform length keeps every label prefix-free, so a keystroke can act at once.
pub(crate) fn hint_labels(count: usize) -> Vec<String> {
    if count == 0 {
        return Vec::new();
    }
    let width = (1u32..)
        .find(|exponent| {
            ALPHABET
                .len()
                .checked_pow(*exponent)
                .is_none_or(|capacity| capacity >= count)
        })
        .unwrap_or(1) as usize;
    (0..count)
        .map(|index| {
            let mut digits = vec![ALPHABET[0]; width];
            let mut rest = index;
            for slot in digits.iter_mut().rev() {
                *slot = ALPHABET[rest % ALPHABET.len()];
                rest /= ALPHABET.len();
            }
            digits.into_iter().collect()
        })
        .collect()
}

/// Hands out labels in document order while the body is built, and records what
/// each one opens.
///
/// The body is built twice: [`Self::counting`] tallies the targets, fixing the
/// label width, then a sized labeller lays the chips in. The count comes from
/// the labeller and not from the link hits because a target can ask for a label
/// without leaving a hit — an empty link name, or an image inside a link.
pub(super) struct HintLabeller {
    /// `None` while counting: requests are tallied and answered with no chip, so
    /// the counting walk lays out exactly like an unhinted body.
    labels: Option<Vec<String>>,
    next: usize,
    /// What has been typed so far, so a chip already ruled out can say so.
    pending: String,
    assigned: Vec<(String, ReaderLinkTarget)>,
}

/// A chip to lay into the body, just past the target it labels: reads as
/// `link │ press a │`, a railed key on a slight background.
pub(super) struct Chip {
    label: String,
    /// Whether the label is still a candidate for what has been typed. A chip
    /// ruled out goes quiet but keeps its width, so the body never reflows
    /// under a keystroke.
    live: bool,
}

impl Chip {
    /// The runs this chip contributes, in order, as `(text, style)`.
    ///
    /// Leads with a plain space so the rail never abuts the name. The fill is
    /// `hover`, which layers under existing ink and so needs no contrast
    /// foreground; a theme that leaves it unset still gets the rails.
    pub(super) fn runs(&self, theme: &Theme) -> Vec<(String, Style)> {
        let lift = if self.live {
            theme.hover()
        } else {
            Style::default()
        };
        let rail = lift.patch(theme.muted());
        let key = if self.live {
            lift.patch(theme.md_link())
                .remove_modifier(Modifier::UNDERLINED)
                .add_modifier(Modifier::BOLD)
        } else {
            rail
        };
        vec![
            (" ".to_string(), Style::default()),
            ("│ press ".to_string(), rail),
            (self.label.clone(), key),
            (" │".to_string(), rail),
        ]
    }

    /// Cells this chip adds to the line, for tests and width reasoning.
    #[cfg(test)]
    fn width(&self) -> usize {
        use unicode_width::UnicodeWidthStr;
        self.runs(&Theme::terminal_default())
            .iter()
            .map(|(text, _)| text.width())
            .sum()
    }
}

impl HintLabeller {
    pub(super) fn new(count: usize, pending: &str) -> Self {
        Self {
            labels: Some(hint_labels(count)),
            next: 0,
            pending: pending.to_string(),
            assigned: Vec::new(),
        }
    }

    /// A labeller that hands out no chips and only tallies the requests, for the
    /// counting walk.
    pub(super) fn counting() -> Self {
        Self {
            labels: None,
            next: 0,
            pending: String::new(),
            assigned: Vec::new(),
        }
    }

    /// The next chip for `target`, or `None` while counting and once the count
    /// the labeller was built for is exhausted.
    pub(super) fn take(&mut self, target: ReaderLinkTarget) -> Option<Chip> {
        self.next += 1;
        let label = self.labels.as_ref()?.get(self.next - 1)?.clone();
        let live = label.starts_with(&self.pending);
        self.assigned.push((label.clone(), target));
        Some(Chip { label, live })
    }

    /// How many labels were asked for, whatever was handed back.
    pub(super) fn requested(&self) -> usize {
        self.next
    }

    pub(super) fn into_assigned(self) -> Vec<(String, ReaderLinkTarget)> {
        self.assigned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_single_letters_up_to_the_alphabet() {
        assert_eq!(hint_labels(3), vec!["a", "s", "d"]);
        assert_eq!(hint_labels(26).len(), 26);
        assert!(hint_labels(26).iter().all(|label| label.len() == 1));
    }

    #[test]
    fn labels_step_up_to_two_characters_past_the_alphabet() {
        let labels = hint_labels(27);
        assert_eq!(labels.len(), 27);
        assert!(labels.iter().all(|label| label.len() == 2));
        assert_eq!(labels[0], "aa");
        assert_eq!(labels[26], "sa");
    }

    #[test]
    fn no_label_is_a_prefix_of_another() {
        for count in [1, 26, 27, 676, 677] {
            let labels = hint_labels(count);
            let width = labels[0].len();
            assert!(labels.iter().all(|label| label.len() == width));
            assert_eq!(
                labels
                    .iter()
                    .collect::<std::collections::HashSet<_>>()
                    .len(),
                labels.len()
            );
        }
    }

    #[test]
    fn labels_are_empty_for_no_targets() {
        assert!(hint_labels(0).is_empty());
    }

    /// A chip's runs join into `link │ press a │`, led by a plain space so the
    /// rail never abuts the name it labels.
    #[test]
    fn a_chip_reads_as_a_railed_click_key() {
        let mut labeller = HintLabeller::new(1, "");
        let chip = labeller.take(ReaderLinkTarget::Image(0)).expect("a chip");
        let text: String = chip
            .runs(&Theme::terminal_default())
            .iter()
            .map(|(text, _)| text.as_str())
            .collect();
        assert_eq!(text, " │ press a │");
    }

    #[test]
    fn a_labeller_records_what_each_label_opens_in_order() {
        let mut labeller = HintLabeller::new(2, "");
        assert!(
            labeller
                .take(ReaderLinkTarget::Uri("https://one".into()))
                .is_some()
        );
        assert!(labeller.take(ReaderLinkTarget::Image(0)).is_some());
        // Built for two targets, so a third finds nothing left to hand out.
        assert!(labeller.take(ReaderLinkTarget::Image(1)).is_none());
        assert_eq!(
            labeller.into_assigned(),
            vec![
                ("a".to_string(), ReaderLinkTarget::Uri("https://one".into())),
                ("s".to_string(), ReaderLinkTarget::Image(0)),
            ]
        );
    }

    /// Typing narrows the field: chips that can no longer match go quiet, but
    /// keep their width so the body never reflows under the keystroke.
    #[test]
    fn a_typed_prefix_rules_out_the_chips_it_excludes() {
        let mut labeller = HintLabeller::new(27, "a");
        let first = labeller.take(ReaderLinkTarget::Image(0)).expect("aa");
        assert!(first.live);
        for index in 1..26 {
            let chip = labeller.take(ReaderLinkTarget::Image(index)).expect("chip");
            assert!(chip.live, "every a? label stays in play");
        }
        let ruled_out = labeller.take(ReaderLinkTarget::Image(26)).expect("sa");
        assert!(!ruled_out.live);
        assert_eq!(
            ruled_out.width(),
            first.width(),
            "a ruled-out chip keeps its width, so nothing reflows"
        );
    }
}
