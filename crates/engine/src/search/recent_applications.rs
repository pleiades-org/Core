//! With nothing typed, the launcher shows recently used apps as a grid. The launcher decides
//! what "recent" means and passes the identifiers, most recent first; search only resolves them
//! against the catalog, so apps that have been uninstalled simply drop out.
use super::{Action, ResultKind, SearchBatch, SearchResult};
use crate::{applications::ApplicationCatalog, RECENT_APPLICATION_LIMIT};
use std::sync::Arc;

const MESSAGE: &str = "Recently used · Enter to open · arrow keys to move · type to search";

/// `None` when no recent identifier is in the catalog, so the usual list is shown instead.
pub(super) fn recent_results(
    recent: &[Arc<str>],
    catalog: &ApplicationCatalog,
) -> Option<SearchBatch> {
    let mut results: Vec<SearchResult> = Vec::new();
    // Only the short recent list is walked; the catalog resolves each identifier to its first
    // entry, since it can list the same identifier twice (a Start Menu shortcut in two folders).
    for identifier in recent {
        if results.len() == RECENT_APPLICATION_LIMIT {
            break;
        }
        let Some(application) = catalog.find(identifier) else {
            continue;
        };
        if results
            .iter()
            .any(|result| Arc::ptr_eq(&result.id, &application.id))
        {
            continue;
        }
        results.push(SearchResult {
            kind: ResultKind::Recent,
            id: application.id.clone(),
            title: application.name.clone(),
            description: application.description.clone(),
            action: Action::LaunchApplication(application.id.clone()),
        });
    }
    (!results.is_empty()).then_some(SearchBatch {
        results,
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
    fn duplicate_identifiers_resolve_to_the_first_catalog_entry_once() {
        let application = |identifier: &str, name: &str| Application {
            id: identifier.into(),
            name: name.into(),
            description: format!("{name} shortcut").into(),
            pinned: false,
            launches: 0,
            aliases: Default::default(),
        };
        // The catalog sorts by name, so "Game (desktop)" is the first entry for `id:game`.
        let catalog = ApplicationCatalog::new(vec![
            application("id:game", "Game (start menu)"),
            application("id:paint", "Paint"),
            application("id:game", "Game (desktop)"),
            application("id:notes", "Notes"),
        ]);
        let batch = recent_results(
            &recent(&["id:notes", "id:game", "id:gone", "id:paint", "id:game"]),
            &catalog,
        )
        .expect("recent apps");
        let titles: Vec<&str> = batch.results.iter().map(|result| &*result.title).collect();
        assert_eq!(titles, ["Notes", "Game (desktop)", "Paint"]);
        assert_eq!(
            catalog.find("id:game").map(|found| &*found.name),
            Some("Game (desktop)")
        );
        assert!(catalog.find("id:gone").is_none());
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
