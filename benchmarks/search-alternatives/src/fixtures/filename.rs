use crate::arena::IndexArena;
use crate::index::CandidateSet;



pub fn middle_out_search(arena: &IndexArena, term: &str, max_results: usize) -> Vec<String> {
    if term.is_empty() || max_results == 0 || arena.files.is_empty() {
        return Vec::new();
    }

    let term_lower = term.to_ascii_lowercase();
    let index = arena.search_index();
    let midpoint = arena.global_midpoint;
    let candidates = index.candidates_for(&term_lower);

    let mut results = Vec::with_capacity(max_results.min(arena.files.len()));
    let mut seen = vec![false; arena.files.len()];

    match candidates {
        CandidateSet::Filtered(list) => {
            collect_matches(
                arena,
                &list,
                &term_lower,
                midpoint,
                max_results,
                &mut results,
                &mut seen,
            );
            if results.len() < max_results {
                collect_matches(
                    arena,
                    &index.visit_order,
                    &term_lower,
                    midpoint,
                    max_results,
                    &mut results,
                    &mut seen,
                );
            }
        }
        CandidateSet::MiddleOut(list) => {
            collect_graph_matches(
                arena,
                &term_lower,
                midpoint,
                max_results,
                &mut results,
                &mut seen,
            );
            if results.len() < max_results {
                collect_matches(
                    arena,
                    list,
                    &term_lower,
                    midpoint,
                    max_results,
                    &mut results,
                    &mut seen,
                );
            }
        }
    }

    results
}

fn collect_graph_matches(
    arena: &IndexArena,
    term_lower: &str,
    midpoint: u16,
    max_results: usize,
    results: &mut Vec<String>,
    seen: &mut [bool],
) {
    let term_len = term_lower.chars().count().min(u16::MAX as usize) as u16;
    let mut stack = vec![(0u32, distance_to_interval(midpoint, arena.dirs[0].subtree_min_len, arena.dirs[0].subtree_max_len))];

    while let Some((dir_id, _)) = stack.pop() {
        if results.len() >= max_results {
            break;
        }

        let dir = &arena.dirs[dir_id as usize];
        if !subtree_may_match(dir.subtree_min_len, dir.subtree_max_len, term_len) {
            continue;
        }

        let mut children: Vec<(u32, u16)> = dir
            .child_dirs
            .iter()
            .map(|&child_id| {
                let child = &arena.dirs[child_id as usize];
                (
                    child_id,
                    distance_to_interval(midpoint, child.subtree_min_len, child.subtree_max_len),
                )
            })
            .collect();
        children.sort_by_key(|(_, dist)| *dist);

        for (child_id, _) in children {
            stack.push((child_id, 0));
        }

        let mut files: Vec<(u32, u16)> = dir
            .file_ids
            .iter()
            .map(|&file_id| {
                let file = &arena.files[file_id as usize];
                (file_id, file.name_len.abs_diff(midpoint))
            })
            .collect();
        files.sort_by_key(|(_, dist)| *dist);

        for (file_id, _) in files {
            let id = file_id as usize;
            if seen[id] {
                continue;
            }
            let file = &arena.files[id];
            if file.name_lower.contains(term_lower) {
                seen[id] = true;
                results.push(file.path.clone());
                if results.len() >= max_results {
                    return;
                }
            }
        }
    }
}

fn collect_matches(
    arena: &IndexArena,
    order: &[u32],
    term_lower: &str,
    midpoint: u16,
    max_results: usize,
    results: &mut Vec<String>,
    seen: &mut [bool],
) {
    let mut prioritized: Vec<(u16, u32)> = order
        .iter()
        .filter_map(|&file_id| {
            let id = file_id as usize;
            if seen[id] {
                return None;
            }
            let file = &arena.files[id];
            Some((file.name_len.abs_diff(midpoint), file_id))
        })
        .collect();
    prioritized.sort_by_key(|(dist, _)| *dist);

    for (_, file_id) in prioritized {
        let id = file_id as usize;
        if seen[id] {
            continue;
        }
        seen[id] = true;
        let file = &arena.files[id];
        if file.name_lower.contains(term_lower) {
            results.push(file.path.clone());
            if results.len() >= max_results {
                break;
            }
        }
    }
}

fn distance_to_interval(target: u16, min_len: u16, max_len: u16) -> u16 {
    if target < min_len {
        min_len - target
    } else if target > max_len {
        target - max_len
    } else {
        0
    }
}

fn subtree_may_match(min_len: u16, max_len: u16, term_len: u16) -> bool {
    if term_len == 0 {
        return true;
    }
    max_len + 8 >= term_len || min_len <= term_len
}


