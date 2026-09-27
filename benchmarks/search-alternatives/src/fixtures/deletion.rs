use crate::arena::IndexArena;
use std::path::{Path, PathBuf};
fn collect_paths(_: &Path) -> Vec<PathBuf> { panic!("fixture only tests a deleted, nonexistent directory") }
pub fn rebuild_subtree(root: &Path, changed_dir: &Path, arena: &mut IndexArena) {
    let root = root.to_path_buf();
    let changed = changed_dir.to_path_buf();

    let stale: Vec<String> = arena
        .files
        .iter()
        .filter(|f| path_within(&f.path, &changed))
        .map(|f| f.path.clone())
        .collect();

    if !stale.is_empty() {
        let stale_set: std::collections::HashSet<String> = stale.into_iter().collect();
        arena.files.retain(|f| !stale_set.contains(&f.path));
        for dir in &mut arena.dirs {
            dir.file_ids.retain(|&id| {
                arena
                    .files
                    .get(id as usize)
                    .map(|f| !stale_set.contains(&f.path))
                    .unwrap_or(false)
            });
        }
    }

    let fresh = if changed.is_dir() {
        collect_paths(&changed)
    } else {
        Vec::new()
    };

    if !fresh.is_empty() {
        let existing: Vec<PathBuf> = arena.files.iter().map(|f| f.path.clone().into()).collect();
        let mut all = existing;
        all.extend(fresh);
        *arena = IndexArena::from_paths(root, all);
    } else {
        arena.recompute_stats();
    }
}

fn path_within(path: &str, dir: &Path) -> bool {
    let path = Path::new(path);
    path.starts_with(dir)
}
