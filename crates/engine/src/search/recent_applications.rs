//! With nothing typed, the launcher shows recently used apps as a grid. The launcher decides
//! what "recent" means and passes the identifiers, most recent first; search only resolves them
//! against the catalog, so apps that have been uninstalled simply drop out.
use super::{Action, ResultKind, SearchBatch, SearchResult};
use crate::{applications::ApplicationCatalog, RECENT_APPLICATION_LIMIT};
use std::{collections::HashMap, sync::Arc};

const MESSAGE: &str = "Recently used · Enter to open · arrow keys to move · type to search";

/// `None` when no recent identifier is in the catalog, so the usual list is shown instead.
pub(super) fn recent_results(
    recent: &[Arc<str>],
    catalog: &ApplicationCatalog,
) -> Option<SearchBatch> {
    let wanted: HashMap<&str, usize> = recent
        .iter()
        .enumerate()
        .map(|(order, identifier)| (&**identifier, order))
        .collect();
    let mut found: Vec<(usize, SearchResult)> = catalog
        .entries()
        .filter_map(|application| {
            let order = *wanted.get(&*application.id)?;
            Some((
                order,
                SearchResult {
                    kind: ResultKind::Recent,
                    id: application.id.clone(),
                    title: application.name.clone(),
                    description: application.description.clone(),
                    action: Action::LaunchApplication(application.id.clone()),
                },
            ))
        })
        .collect();
    if found.is_empty() {
        return None;
    }
    found.sort_unstable_by_key(|(order, _)| *order);
    // A catalog can list the same identifier twice (a Start Menu shortcut in two folders).
    found.dedup_by_key(|(order, _)| *order);
    Some(SearchBatch {
        results: found
            .into_iter()
            .take(RECENT_APPLICATION_LIMIT)
            .map(|(_, result)| result)
            .collect(),
        message: MESSAGE,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::applications::Application;

    fn catalog(names: &[&str]) -> ApplicationCatalog {
        ApplicationCatalog::new(
            names
                .iter()
                .map(|name| Application {
                    id: format!("id:{name}").into(),
                    name: (*name).into(),
                    description: "Programs".into(),
                    pinned: false,
                    launches: 0,
                    aliases: Default::default(),
                })
                .collect(),
        )
    }

    fn recent(identifiers: &[&str]) -> Vec<Arc<str>> {
        identifiers
            .iter()
            .map(|identifier| Arc::from(*identifier))
            .collect()
    }

    #[test]
    fn recent_apps_keep_their_order_and_skip_unknown_ones() {
        let catalog = catalog(&["Calculator", "Notepad", "Paint"]);
        let batch = recent_results(&recent(&["id:Paint", "id:Gone", "id:Calculator"]), &catalog)
            .expect("recent apps");
        let titles: Vec<&str> = batch.results.iter().map(|result| &*result.title).collect();
        assert_eq!(titles, ["Paint", "Calculator"]);
        assert!(batch
            .results
            .iter()
            .all(|result| result.kind == ResultKind::Recent
                && result.action == Action::LaunchApplication(result.id.clone())));
    }

    #[test]
    fn nothing_recent_in_the_catalog_falls_back() {
        assert!(recent_results(&recent(&["id:Gone"]), &catalog(&["Notepad"])).is_none());
        assert!(recent_results(&[], &catalog(&["Notepad"])).is_none());
    }

    #[test]
    fn the_grid_is_capped() {
        let names: Vec<String> = (0..30).map(|index| format!("App {index:02}")).collect();
        let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let identifiers: Vec<String> = names.iter().map(|name| format!("id:{name}")).collect();
        let identifier_refs: Vec<&str> = identifiers.iter().map(String::as_str).collect();
        let batch =
            recent_results(&recent(&identifier_refs), &catalog(&name_refs)).expect("recent apps");
        assert_eq!(batch.results.len(), RECENT_APPLICATION_LIMIT);
        assert_eq!(&*batch.results[0].title, "App 00");
    }
}
