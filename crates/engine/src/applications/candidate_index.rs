const MIN_INDEXED_APPLICATIONS: usize = 128;
const INDEX_BYTE_BUDGET: usize = 512 * 1024;
const BUILD_PAIR_LIMIT: usize = 256 * 1024;

#[derive(Debug)]
struct GramRange {
    gram: u32,
    start: u32,
    length: u32,
}

/// Compact immutable postings. Candidates are always verified by the complete ranker.
#[derive(Debug)]
pub(super) struct CandidateIndex {
    ranges: Box<[GramRange]>,
    postings: Box<[u32]>,
}

impl CandidateIndex {
    pub fn build<'name>(names: impl ExactSizeIterator<Item = &'name str>) -> Option<Self> {
        if names.len() < MIN_INDEXED_APPLICATIONS || names.len() > u32::MAX as usize {
            return None;
        }
        let posting_budget =
            INDEX_BYTE_BUDGET.checked_sub(names.len() * std::mem::size_of::<usize>())?;
        let mut pairs = Vec::new();
        for (ordinal, name) in names.enumerate() {
            for bytes in name.as_bytes().windows(3) {
                if pairs.len() == BUILD_PAIR_LIMIT {
                    return None;
                }
                pairs.push((gram(bytes), ordinal as u32));
            }
        }
        pairs.sort_unstable();
        pairs.dedup();
        let mut ranges: Vec<GramRange> = Vec::new();
        let mut postings = Vec::with_capacity(pairs.len());
        for (gram, ordinal) in pairs {
            if let Some(range) = ranges.last_mut().filter(|range| range.gram == gram) {
                range.length += 1;
            } else {
                ranges.push(GramRange {
                    gram,
                    start: postings.len() as u32,
                    length: 1,
                });
            }
            postings.push(ordinal);
            if ranges.len() * std::mem::size_of::<GramRange>() + postings.len() * 4 > posting_budget
            {
                return None;
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

    /// None requests a scan for short words; an empty slice proves no strict match.
    /// Grams come from individual terms so reordered word queries remain complete.
    pub fn candidates(&self, query: &str) -> Option<&[u32]> {
        let mut shortest: Option<&[u32]> = None;
        for bytes in query
            .split_whitespace()
            .flat_map(|word| word.as_bytes().windows(3))
        {
            let Ok(position) = self
                .ranges
                .binary_search_by_key(&gram(bytes), |range| range.gram)
            else {
                return Some(&[]);
            };
            let range = &self.ranges[position];
            let posting =
                &self.postings[range.start as usize..(range.start + range.length) as usize];
            if shortest.is_none_or(|current| posting.len() < current.len()) {
                shortest = Some(posting);
            }
        }
        shortest
    }
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
}
