use std::{cmp::Reverse, collections::HashMap};

const MIN_INDEXED_APPLICATIONS: usize = 128;
const INDEX_BYTE_BUDGET: usize = 512 * 1024;
const BIGRAM: u32 = 1 << 24;
/// Marks a gram left out of the postings because so many names contain it. The top bit is
/// separate from the gram bytes and the bigram tag.
const UNINDEXED: u32 = 1 << 31;

/// Where a gram's postings start; they end where the next gram's start.
#[derive(Debug)]
struct GramRange {
    gram: u32,
    start: u32,
}

/// Compact immutable postings. Candidates are always verified by the complete ranker.
#[derive(Debug)]
pub(super) struct CandidateIndex {
    ranges: Box<[GramRange]>,
    /// 16-bit ordinals. A catalog too large for them would not fit the budget with 32-bit ones
    /// either: its four-byte initials order alone would take half.
    postings: Box<[u16]>,
}

impl CandidateIndex {
    pub fn build<'name>(names: impl ExactSizeIterator<Item = &'name str> + Clone) -> Option<Self> {
        if names.len() < MIN_INDEXED_APPLICATIONS || names.len() > usize::from(u16::MAX) {
            return None;
        }
        // The catalog's initials order shares the budget: four bytes per name.
        let budget = INDEX_BYTE_BUDGET.checked_sub(names.len() * std::mem::size_of::<u32>())?;
        let counts = count_grams(names.clone(), budget)?;
        let unindexed = common_grams(&counts, budget)?;
        Some(Self::with_postings(names, &counts, &unindexed))
    }

    /// Allocate only the postings that fit, then fill each range in catalog order.
    fn with_postings<'name>(
        names: impl Iterator<Item = &'name str>,
        counts: &[(u32, usize)],
        unindexed: &[bool],
    ) -> Self {
        let mut ranges = Vec::with_capacity(counts.len());
        let mut cursors = HashMap::new();
        let mut posting_count = 0;
        for (&(gram, count), &unindexed) in counts.iter().zip(unindexed) {
            ranges.push(GramRange {
                gram: if unindexed { gram | UNINDEXED } else { gram },
                start: posting_count as u32,
            });
            if !unindexed {
                cursors.insert(gram, (posting_count, usize::MAX));
                posting_count += count;
            }
        }
        let mut postings = vec![0; posting_count];
        for (ordinal, name) in names.enumerate() {
            for gram in name_grams(name) {
                if let Some((cursor, last_ordinal)) = cursors.get_mut(&gram) {
                    if *last_ordinal != ordinal {
                        postings[*cursor] = ordinal as u16;
                        *cursor += 1;
                        *last_ordinal = ordinal;
                    }
                }
            }
        }
        Self {
            ranges: ranges.into_boxed_slice(),
            postings: postings.into_boxed_slice(),
        }
    }

    pub fn memory_bytes(&self) -> usize {
        std::mem::size_of_val(&*self.ranges) + std::mem::size_of_val(&*self.postings)
    }

    /// None requests another path: single-byte words, or only grams too common to index. An empty
    /// slice proves no strict match. Grams come from individual terms so reordered word queries
    /// remain complete.
    pub fn candidates(&self, query: &str) -> Option<&[u16]> {
        let mut shortest: Option<&[u16]> = None;
        for gram in query.split_whitespace().flat_map(query_grams) {
            let Ok(position) = self
                .ranges
                .binary_search_by_key(&gram, |range| range.gram & !UNINDEXED)
            else {
                return Some(&[]);
            };
            let range = &self.ranges[position];
            // Any name may contain it, so it narrows nothing.
            if range.gram & UNINDEXED != 0 {
                continue;
            }
            let end = self
                .ranges
                .get(position + 1)
                .map_or(self.postings.len(), |next| next.start as usize);
            let posting = &self.postings[range.start as usize..end];
            if shortest.is_none_or(|current| posting.len() < current.len()) {
                shortest = Some(posting);
            }
        }
        shortest
    }
}

/// Count each gram once per name without retaining raw pairs. Only the distinct ranges can
/// exhaust this budget; long names and repeated grams do not turn off the whole index.
fn count_grams<'name>(
    names: impl Iterator<Item = &'name str>,
    budget: usize,
) -> Option<Vec<(u32, usize)>> {
    let mut counts = HashMap::new();
    for (ordinal, name) in names.enumerate() {
        for gram in name_grams(name) {
            let (count, last_ordinal) = counts.entry(gram).or_insert((0, usize::MAX));
            if *last_ordinal != ordinal {
                *count += 1;
                *last_ordinal = ordinal;
            }
            if counts.len() > budget / std::mem::size_of::<GramRange>() {
                return None;
            }
        }
    }
    let mut counts: Vec<_> = counts
        .into_iter()
        .map(|(gram, (count, _))| (gram, count))
        .collect();
    counts.sort_unstable_by_key(|&(gram, _)| gram);
    Some(counts)
}

fn name_grams(name: &str) -> impl Iterator<Item = u32> + '_ {
    name.as_bytes()
        .windows(3)
        .map(gram)
        .chain(name.as_bytes().windows(2).map(gram))
}

fn query_grams(word: &str) -> impl Iterator<Item = u32> + '_ {
    let width = if word.len() == 2 { 2 } else { 3 };
    word.as_bytes().windows(width).map(gram)
}

/// Which grams to leave out, most common first, so ranges and postings fit `budget` bytes.
/// Every gram keeps its range, so an absent gram still proves no match. `None` if even the
/// ranges alone do not fit.
fn common_grams(counts: &[(u32, usize)], budget: usize) -> Option<Vec<bool>> {
    let range_bytes = counts.len() * std::mem::size_of::<GramRange>();
    let mut posting_bytes =
        counts.iter().map(|(_, count)| count).sum::<usize>() * std::mem::size_of::<u16>();
    let mut unindexed = vec![false; counts.len()];
    if range_bytes + posting_bytes > budget {
        let mut most_common: Vec<usize> = (0..counts.len()).collect();
        // Equal-frequency bigrams narrow less than trigrams and have the prefix fallback.
        // Drop them first so adding short-query coverage does not displace useful trigrams.
        most_common.sort_unstable_by_key(|&position| {
            (
                Reverse(counts[position].1),
                Reverse(counts[position].0 & BIGRAM),
                position,
            )
        });
        for position in most_common {
            if range_bytes + posting_bytes <= budget {
                break;
            }
            unindexed[position] = true;
            posting_bytes -= counts[position].1 * std::mem::size_of::<u16>();
        }
    }
    (range_bytes + posting_bytes <= budget).then_some(unindexed)
}

fn gram(bytes: &[u8]) -> u32 {
    let suffix = bytes.get(2).map_or(BIGRAM, |&byte| u32::from(byte) << 16);
    u32::from(bytes[0]) | (u32::from(bytes[1]) << 8) | suffix
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiny_catalogs_scan_but_long_repeated_names_keep_an_index() {
        assert!(CandidateIndex::build(["small app"].into_iter()).is_none());
        let long_name = "x".repeat(3000);
        let index = CandidateIndex::build(std::iter::repeat_n(long_name.as_str(), 128)).unwrap();
        assert_eq!(index.candidates("xxx").unwrap().len(), 128);
        assert_eq!(index.candidates("xx").unwrap().len(), 128);
        assert_eq!(index.candidates("xz"), Some(&[][..]));
    }

    #[test]
    fn the_most_common_grams_are_left_out_to_fit_the_budget() {
        // Long names exceed the old raw-pair limit and the retained posting budget, while
        // their rare grams still fit alongside the initials order.
        let names: Vec<String> = (0..10_000)
            .map(|index| format!("portableofficesuiteprofessionalextendededition{index:05}"))
            .collect();
        let index = CandidateIndex::build(names.iter().map(String::as_str)).expect("index");
        assert!(index.memory_bytes() + names.len() * 4 <= INDEX_BYTE_BUDGET);
        let unindexed: Vec<String> = index
            .ranges
            .iter()
            .filter(|range| range.gram & UNINDEXED != 0)
            .map(|range| {
                let width = if range.gram & BIGRAM != 0 { 2 } else { 3 };
                String::from_utf8(range.gram.to_le_bytes()[..width].to_vec()).unwrap()
            })
            .collect();
        assert!(!unindexed.is_empty());
        // Every name contains them, so they narrow nothing; rarer grams still do.
        for gram in &unindexed {
            assert_eq!(index.candidates(gram), None, "{gram}");
        }
        assert!(index
            .candidates("edition01234")
            .is_some_and(|posting| posting.len() < 200 && posting.contains(&1234)));
        // An absent gram still proves there is no match.
        assert_eq!(index.candidates("zzz"), Some(&[][..]));
    }

    #[test]
    fn range_lengths_come_from_the_next_start() {
        let names: Vec<String> = (0..200).map(|index| format!("app {index:03}")).collect();
        let index = CandidateIndex::build(names.iter().map(String::as_str)).expect("index");
        assert_eq!(std::mem::size_of::<GramRange>(), 8);
        assert_eq!(index.candidates("app").map(<[u16]>::len), Some(200));
        assert_eq!(index.candidates("199").map(<[u16]>::len), Some(1));
        // `pp ` is followed by the digit grams; the last gram ends with the postings.
        let last = index.ranges.last().unwrap().gram & !UNINDEXED;
        let width = if last & BIGRAM != 0 { 2 } else { 3 };
        let last = String::from_utf8(last.to_le_bytes()[..width].to_vec()).unwrap();
        assert!(index
            .candidates(&last)
            .is_some_and(|posting| !posting.is_empty()));
    }
}
