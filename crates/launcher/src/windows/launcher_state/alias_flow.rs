//! Aliases in what is typed: Settings → Aliases gives short names to longer text, and Core
//! acts on the longer text while the search box keeps what was typed.
use super::LauncherState;
use core_engine::aliases::expand;

impl LauncherState {
    /// `typed` as Core acts on it: a leading alias replaced by what it stands for.
    pub(super) fn expand_aliases(&self, typed: &str) -> String {
        expand(typed, &self.settings.saved.aliases).into_owned()
    }

    /// The search box's text as Core acts on it. Code that edits the box, such as the command
    /// prompt's history, reads the box itself instead.
    pub(super) fn acted_query(&self) -> String {
        let typed = self
            .view
            .as_ref()
            .map(|view| view.query())
            .unwrap_or_default();
        self.expand_aliases(&typed)
    }
}
