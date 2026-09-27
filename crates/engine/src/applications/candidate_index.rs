use std::cmp::Reverse;

const MIN_INDEXED_APPLICATIONS: usize = 128;
const INDEX_BYTE_BUDGET: usize = 512 * 1024;
const BUILD_PAIR_LIMIT: usize = 256 * 1024;
/// Marks a gram left out of the postings because so many names contain it. Grams use only the
/// low three bytes, so the top bit is free.
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
    pub fn build<'name>(names: impl ExactSizeIterator<Item = &'name str>) -> Option<Self> {
        if names.len() < MIN_INDEXED_APPLICATIONS || names.len() > usize::from(u16::MAX) {
            return None;
        }
        // The catalog's initials order shares the budget: four bytes per name.
        let budget = INDEX_BYTE_BUDGET.checked_sub(names.len() * std::mem::size_of::<u32>())?;
        let mut pairs = Vec::new();
        for (ordinal, name) in names.enumerate() {
            for bytes in name.as_bytes().windows(3) {
                if pairs.len() == BUILD_PAIR_LIMIT {
                    return None;
                }
                pairs.push((gram(bytes), ordinal as u16));
            }
        }
        pairs.sort_unstable();
        pairs.dedup();
        let mut counts: Vec<(u32, usize)> = Vec::new();
        for &(gram, _) in &pairs {
            match counts.last_mut() {
                Some((last, count)) if *last == gram => *count += 1,
                _ => counts.push((gram, 1)),
            }
        }
        let unindexed = common_grams(&counts, budget)?;
        let mut ranges = Vec::with_capacity(counts.len());
        let mut postings = Vec::with_capacity(pairs.len());
        let mut pairs = pairs.into_iter();
        for (&(gram, count), &unindexed) in counts.iter().zip(&unindexed) {
            ranges.push(GramRange {
                gram: if unindexed { gram | UNINDEXED } else { gram },
                start: postings.len() as u32,
            });
            let ordinals = pairs.by_ref().take(count).map(|(_, ordinal)| ordinal);
            if unindexed {
                ordinals.for_each(drop);
            } else {
                postings.extend(ordinals);
            }
        }
        Some(Self {
            ranges: ranges.into_boxed_slice(),
            postings: postings.into_boxed_slice(),
        })
    }

    pub fn memory_bytes(&self) -> usize {
        std::mem::size_of_val(&*self.ranges) + std::mem::size_of_val(&*self.postings)
    }

    /// None requests another path: short words, or only grams too common to index. An empty
    /// slice proves no strict match. Grams come from individual terms so reordered word queries
    /// remain complete.
    pub fn candidates(&self, query: &str) -> Option<&[u16]> {
        let mut shortest: Option<&[u16]> = None;
        for bytes in query
            .split_whitespace()
            .flat_map(|word| word.as_bytes().windows(3))
        {
            let Ok(position) = self
                .ranges
                .binary_search_by_key(&gram(bytes), |range| range.gram & !UNINDEXED)
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
        most_common.sort_unstable_by_key(|&position| (Reverse(counts[position].1), position));
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
    u32::from(bytes[0]) | (u32::from(bytes[1]) << 8) | (u32::from(bytes[2]) << 16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiny_catalogs_and_excessive_build_work_use_the_scan_fallback() {
        assert!(CandidateIndex::build(["small app"].into_iter()).is_none());
        let long_name = "x".repeat(3000);
        assert!(CandidateIndex::build(std::iter::repeat_n(long_name.as_str(), 128)).is_none());
    }

    #[test]
    fn the_most_common_grams_are_left_out_to_fit_the_budget() {
        // 25 grams per name: about 500 KiB of postings, over the 472 KiB left beside the
        // initials order.
        let names: Vec<String> = (0..10_000)
            .map(|index| format!("portableofficesuitepro{index:05}"))
            .collect();
        let index = CandidateIndex::build(names.iter().map(String::as_str)).expect("index");
        assert!(index.memory_bytes() + names.len() * 4 <= INDEX_BYTE_BUDGET);
        let unindexed: Vec<String> = index
            .ranges
            .iter()
            .filter(|range| range.gram & UNINDEXED != 0)
            .map(|range| String::from_utf8(range.gram.to_le_bytes()[..3].to_vec()).unwrap())
            .collect();
        assert!(!unindexed.is_empty());
        // Every name contains them, so they narrow nothing; rarer grams still do.
        for gram in &unindexed {
            assert_eq!(index.candidates(gram), None, "{gram}");
        }
        assert!(index.candidates("portable").is_some());
        assert!(index
            .candidates("pro01234")
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
        let last = String::from_utf8(last.to_le_bytes()[..3].to_vec()).unwrap();
        assert!(index
            .candidates(&last)
            .is_some_and(|posting| !posting.is_empty()));
    }
}
