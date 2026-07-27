//! Counting for the filter browser's facet tabs.
//!
//! A tag/person/activity/feeling row launches an exact search for the value it
//! shows, so its count is a [`FacetTally`] taken while walking the entries once —
//! not a search per row. Place rows keep containment, because a place contains
//! the places around it, so [`PlaceCounter`] answers them by address instead.

use std::collections::{BTreeSet, HashMap};

use notema_domain::{Location, PlaceGroup};

use crate::tui::features::search::location_tokens;

/// One case-folded value: how many entries carry it, and the stamp that keeps an
/// entry from counting twice.
#[derive(Default)]
struct Bucket {
    entries: usize,
    /// The entry ordinal that last counted this bucket. Ordinals start at 1, so
    /// the default 0 reads as "not yet counted".
    stamp: u32,
}

/// Where one original casing lands, and how often it was written that way.
struct Form {
    bucket: u32,
    votes: usize,
}

/// A tally of one facet's distinct values across the entries in scope.
#[derive(Default)]
pub(crate) struct FacetTally {
    /// Keyed on the value as stored, so the hot path hashes a borrowed `&str` and
    /// the fold below runs once per distinct value rather than once per entry
    /// carrying it.
    forms: HashMap<String, Form>,
    /// Folded value to its bucket. `to_lowercase`, not `eq_ignore_ascii_case`: the
    /// search folds the same way, and an ASCII-only fold would show `Ärger` and
    /// `ärger` as one row whose own search returned both.
    keys: HashMap<String, u32>,
    buckets: Vec<Bucket>,
    ordinal: u32,
}

impl FacetTally {
    /// Add one entry's values for this facet. Call once per entry, including for
    /// entries carrying none, so the ordinals stay distinct.
    pub(crate) fn add_entry(&mut self, values: &[String]) {
        self.ordinal += 1;
        for value in values {
            self.add(value);
        }
    }

    fn add(&mut self, value: &str) {
        if let Some(form) = self.forms.get_mut(value) {
            form.votes += 1;
            let bucket = form.bucket;
            self.count(bucket);
            return;
        }
        let key = value.to_lowercase();
        // A value that folds to nothing gets no row: the parser drops an empty
        // needle, so its own search would match nothing.
        if key.is_empty() {
            return;
        }
        let bucket = match self.keys.get(key.as_str()) {
            Some(&bucket) => bucket,
            None => {
                let bucket = self.buckets.len() as u32;
                self.buckets.push(Bucket::default());
                self.keys.insert(key, bucket);
                bucket
            }
        };
        self.forms
            .insert(value.to_string(), Form { bucket, votes: 1 });
        self.count(bucket);
    }

    fn count(&mut self, bucket: u32) {
        let ordinal = self.ordinal;
        let bucket = &mut self.buckets[bucket as usize];
        // An entry carrying both `Work` and `work` is still one entry.
        if bucket.stamp != ordinal {
            bucket.stamp = ordinal;
            bucket.entries += 1;
        }
    }

    /// Each distinct value as `(display casing, entries)`, unordered.
    ///
    /// The casing is the most-written one, ties going to the lexicographically
    /// smallest form, so `Work` beats `work` at equal use. Votes count
    /// occurrences rather than entries: they only choose a label, and counting
    /// them per occurrence is what keeps every label where the per-tab counter
    /// had it. The number on the row is `entries`, which is entry-deduped.
    pub(crate) fn rows(self) -> Vec<(String, usize)> {
        let mut labels: Vec<Option<(String, usize)>> = vec![None; self.buckets.len()];
        for (form, Form { bucket, votes }) in self.forms {
            let slot = &mut labels[bucket as usize];
            let better = match slot {
                Some((best, best_votes)) => {
                    votes > *best_votes || (votes == *best_votes && form < *best)
                }
                None => true,
            };
            if better {
                *slot = Some((form, votes));
            }
        }
        labels
            .into_iter()
            .zip(self.buckets)
            .map(|(label, bucket)| {
                let (label, _) = label.expect("every bucket was created by a form");
                (label, bucket.entries)
            })
            .collect()
    }
}

/// Counts for the locations tab, whose rows keep containment: a row matches an
/// entry when every word of its query occurs in that entry's address haystack.
///
/// `location_predicate` reads nothing but that haystack, so entries sharing an
/// address are interchangeable to it. Interning them into one weighted address is
/// therefore exactly equivalent, and turns rows x entries substring tests into
/// distinct-words x distinct-addresses — 625 rows over 25k entries becomes ~630
/// words over ~5k addresses on the wide bench corpus.
#[derive(Default)]
pub(crate) struct PlaceCounter {
    groups: BTreeSet<PlaceGroup>,
    /// Distinct address haystack to the number of entries carrying it.
    addresses: HashMap<String, usize>,
}

impl PlaceCounter {
    /// Add one entry by its location. An entry without one contributes nothing
    /// and gets no row, as a predicate requiring a location does.
    pub(crate) fn add_entry(&mut self, location: Option<&Location>) {
        let Some(location) = location else {
            return;
        };
        if let Some(group) = location.place_group() {
            self.groups.insert(group);
        }
        // One `search_haystack` per entry, where a scan per row spent one per
        // entry per row. The string it builds becomes the key, so interning it
        // costs no second allocation.
        *self
            .addresses
            .entry(location.search_haystack())
            .or_default() += 1;
    }

    /// Each place group with the number of entries its `location:` search returns.
    pub(crate) fn counts(self) -> Vec<(PlaceGroup, usize)> {
        let groups: Vec<PlaceGroup> = self.groups.into_iter().collect();

        // Intern the words across every row's query: a country word is shared by
        // every settlement in it, so the distinct set is far smaller than the rows.
        let mut words: Vec<String> = Vec::new();
        let mut ids: HashMap<String, u32> = HashMap::new();
        let per_group: Vec<Vec<u32>> = groups
            .iter()
            .map(|group| {
                let mut group_ids: Vec<u32> = location_tokens(&group.search_query())
                    .into_iter()
                    .map(|word| {
                        *ids.entry(word).or_insert_with_key(|word| {
                            words.push(word.clone());
                            (words.len() - 1) as u32
                        })
                    })
                    .collect();
                group_ids.sort_unstable();
                group_ids.dedup();
                group_ids
            })
            .collect();

        let mut counts = vec![0usize; groups.len()];
        let mut present = vec![false; words.len()];
        for (address, weight) in &self.addresses {
            for (id, word) in words.iter().enumerate() {
                present[id] = address.contains(word);
            }
            for (group, group_ids) in per_group.iter().enumerate() {
                // A query tokenizing to nothing matches nothing, never
                // everything: `all` over an empty set would invert the rule.
                if !group_ids.is_empty() && group_ids.iter().all(|&id| present[id as usize]) {
                    counts[group] += weight;
                }
            }
        }
        groups.into_iter().zip(counts).collect()
    }
}
