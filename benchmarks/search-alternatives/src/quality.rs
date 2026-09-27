use crate::matching::{self, Catalog, Method, Scratch, Selector};

pub fn verify(catalog: &Catalog) {
    for &(_, query) in crate::corpus::QUERIES {
        let mut expected = Scratch::default();
        matching::search(
            catalog,
            query,
            Method::Scan,
            Selector::Partial,
            &mut expected,
        );
        for method in Method::ALL {
            for selector in [Selector::Partial, Selector::Heap, Selector::Fixed] {
                let mut actual = Scratch::default();
                matching::search(catalog, query, method, selector, &mut actual);
                assert_eq!(
                    actual.ranked, expected.ranked,
                    "{method:?}/{selector:?}/{query}"
                );
            }
        }
    }
    for query in ["a", "report", "code visual", "zzqxyw_no_such_file"] {
        let expected = matching::legacy_search(catalog, query);
        for indexed in [false, true] {
            let mut actual = Scratch::default();
            matching::borrowed_core_search(catalog, query, indexed, &mut actual);
            assert_eq!(
                actual
                    .ranked
                    .iter()
                    .map(|result| result.identifier)
                    .collect::<Vec<_>>(),
                expected
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::IndexArena;
    use std::path::PathBuf;

    #[test]
    fn documents_stale_directory_ids_after_upstream_deletion() {
        let root = PathBuf::from("C:/core-benchmark-nonexistent-deletion-fb1d7969");
        assert!(!root.exists(), "fixture must not refer to a real directory");
        let removed = root.join("removed");
        let retained = root.join("retained");
        let mut arena = IndexArena::from_paths(
            root.clone(),
            vec![removed.join("a.txt"), retained.join("b.txt")],
        );
        crate::upstream_deletion::rebuild_subtree(&root, &removed, &mut arena);
        assert_eq!(arena.files.len(), 1);
        assert!(arena.files[0].path.ends_with("b.txt"));
        // Fingerprint the existing defect: b.txt remains attached to the removed directory.
        let removed_node = arena
            .dirs
            .iter()
            .find(|directory| directory.name == "removed")
            .unwrap();
        let retained_node = arena
            .dirs
            .iter()
            .find(|directory| directory.name == "retained")
            .unwrap();
        assert_eq!(removed_node.file_ids, vec![0]);
        assert!(retained_node.file_ids.is_empty());
    }

    #[test]
    fn all_candidate_and_selection_methods_agree_on_complete_pipeline() {
        verify(&Catalog::new(crate::corpus::arena(2_000)));
    }

    #[test]
    fn duplicate_names_keep_distinct_ids_and_deterministic_prefix_ties() {
        let root = PathBuf::from("C:/duplicate-name-fixture");
        let paths = (0..24)
            .map(|identifier| root.join(format!("folder_{identifier}/code.exe")))
            .collect();
        let catalog = Catalog::new(IndexArena::from_paths(root, paths));
        for query in ["code", "code.exe", "exe", "c"] {
            let mut expected = Scratch::default();
            matching::search(
                &catalog,
                query,
                Method::Scan,
                Selector::Partial,
                &mut expected,
            );
            assert_eq!(
                expected
                    .ranked
                    .iter()
                    .map(|result| result.identifier)
                    .collect::<Vec<_>>(),
                (0..12).collect::<Vec<_>>()
            );
            for method in Method::ALL {
                let mut actual = Scratch::default();
                matching::search(&catalog, query, method, Selector::Partial, &mut actual);
                assert_eq!(actual.ranked, expected.ranked, "{method:?}/{query}");
            }
        }
    }

    #[test]
    fn upstream_unlimited_membership_matches_substring_oracle() {
        let arena = crate::corpus::arena(2_000);
        for &(_, query) in crate::corpus::QUERIES {
            let mut expected: Vec<_> = arena
                .files
                .iter()
                .filter(|file| file.name_lower.contains(query))
                .map(|file| file.path.clone())
                .collect();
            let mut actual = crate::upstream::middle_out_search(&arena, query, arena.files.len());
            expected.sort();
            actual.sort();
            assert_eq!(actual, expected, "{query}");
        }
    }

    #[test]
    fn generated_substrings_and_adversarial_queries_preserve_top_results() {
        let catalog = Catalog::new(crate::corpus::arena(800));
        let mut queries = vec![
            String::new(),
            "ZZQ".into(),
            "é".into(),
            "東京".into(),
            "a".repeat(1024),
        ];
        for file in catalog.arena.files.iter().step_by(13) {
            let characters: Vec<_> = file
                .name_lower
                .char_indices()
                .map(|(position, _)| position)
                .chain(std::iter::once(file.name_lower.len()))
                .collect();
            for positions in characters.windows(4) {
                queries.push(file.name_lower[positions[0]..positions[3]].to_owned());
            }
        }
        for query in queries {
            let mut expected = Scratch::default();
            matching::search(
                &catalog,
                &query,
                Method::Scan,
                Selector::Partial,
                &mut expected,
            );
            for method in Method::ALL {
                let mut actual = Scratch::default();
                matching::search(&catalog, &query, method, Selector::Partial, &mut actual);
                assert_eq!(actual.ranked, expected.ranked, "{method:?}: {query}");
            }
        }
    }

    #[test]
    fn incremental_narrowing_handles_backspace_and_does_not_reuse_only_top_results() {
        let arena = crate::corpus::arena(2_000);
        let mut incremental = matching::Incremental::default();
        for query in [
            "r",
            "re",
            "rep",
            "report",
            "re",
            "a",
            "au",
            "audio",
            "",
            "needle_7fd9",
        ] {
            let expected: Vec<_> = arena
                .files
                .iter()
                .enumerate()
                .filter(|(_, file)| file.name_lower.contains(query))
                .map(|(identifier, _)| identifier as u32)
                .collect();
            assert_eq!(incremental.update(&arena, query), expected);
        }
    }

    #[test]
    fn missing_trigram_is_an_empty_candidate_set() {
        let arena = crate::corpus::arena(100);
        assert_eq!(
            crate::postings::shortest_hash("report_zzq", &arena.search),
            Some(&[][..])
        );
    }

    #[test]
    fn empty_catalog_and_zero_results_are_safe() {
        let catalog = Catalog::new(crate::corpus::arena(0));
        verify(&catalog);
        assert!(crate::upstream::middle_out_search(&catalog.arena, "a", 0).is_empty());
    }

    #[test]
    fn document_upstream_ascii_only_case_matching() {
        let arena = IndexArena::from_paths(
            PathBuf::from("C:/fixture"),
            vec![PathBuf::from("C:/fixture/CAFÉ.txt")],
        );
        assert!(crate::upstream::middle_out_search(&arena, "café", 12).is_empty());
        assert_eq!(
            crate::upstream::middle_out_search(&arena, "CAFÉ", 12).len(),
            1
        );
    }
}
