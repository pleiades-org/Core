use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;

pub const RESULT_LIMIT: usize = 12;

pub struct Record {
    pub name: String,
    pub score: u8,
}

pub fn make_records(count: usize) -> Vec<Record> {
    (0..count)
        .map(|index| Record {
            name: format!("application {:06}", (index * 7_919) % count),
            score: ((index * 37) % 97) as u8,
        })
        .collect()
}

pub fn legacy(records: &[Record]) -> Vec<usize> {
    crate::legacy_search_text::take_top_scored(
        0..records.len(),
        |index| Some(records[*index].score),
        RESULT_LIMIT,
        |index| records[*index].name.clone(),
    )
    .into_iter()
    .map(|(_, index)| index)
    .collect()
}

fn compare(records: &[Record], left: usize, right: usize) -> Ordering {
    records[right]
        .score
        .cmp(&records[left].score)
        .then_with(|| records[left].name.cmp(&records[right].name))
}

pub fn borrowed_partial(records: &[Record]) -> Vec<usize> {
    let mut indices: Vec<_> = (0..records.len()).collect();
    if indices.len() > RESULT_LIMIT {
        indices.select_nth_unstable_by(RESULT_LIMIT - 1, |left, right| {
            compare(records, *left, *right)
        });
        indices.truncate(RESULT_LIMIT);
    }
    indices.sort_unstable_by(|left, right| compare(records, *left, *right));
    indices
}

pub fn full_sort(records: &[Record]) -> Vec<usize> {
    let mut indices: Vec<_> = (0..records.len()).collect();
    indices.sort_unstable_by(|left, right| compare(records, *left, *right));
    indices.truncate(RESULT_LIMIT);
    indices
}

pub fn bounded_heap(records: &[Record]) -> Vec<usize> {
    let mut candidates = BinaryHeap::with_capacity(RESULT_LIMIT);
    for (index, record) in records.iter().enumerate() {
        let candidate = (Reverse(record.score), record.name.as_str(), index);
        if candidates.len() < RESULT_LIMIT {
            candidates.push(candidate);
        } else if candidate < *candidates.peek().expect("heap reached its fixed limit") {
            *candidates.peek_mut().expect("heap reached its fixed limit") = candidate;
        }
    }
    candidates
        .into_sorted_vec()
        .into_iter()
        .map(|(_, _, index)| index)
        .collect()
}

pub fn fixed_buffer(records: &[Record]) -> Vec<usize> {
    let mut candidates = [0_usize; RESULT_LIMIT];
    let mut retained = 0;
    for index in 0..records.len() {
        if retained == RESULT_LIMIT && compare(records, index, candidates[retained - 1]).is_ge() {
            continue;
        }
        let position = candidates[..retained]
            .partition_point(|existing| compare(records, *existing, index).is_lt());
        let new_length = (retained + 1).min(RESULT_LIMIT);
        candidates.copy_within(position..new_length - 1, position + 1);
        candidates[position] = index;
        retained = new_length;
    }
    candidates[..retained].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_selectors_return_identical_ordered_top_results() {
        for count in [0, 1, 11, 12, 13, 100, 2_000] {
            let mut records = make_records(count);
            for _ in 0..2 {
                let expected = full_sort(&records);
                assert_eq!(legacy(&records), expected);
                assert_eq!(borrowed_partial(&records), expected);
                assert_eq!(bounded_heap(&records), expected);
                assert_eq!(fixed_buffer(&records), expected);
                records.reverse();
            }
        }
    }
}
