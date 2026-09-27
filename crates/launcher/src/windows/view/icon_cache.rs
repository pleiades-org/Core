//! Icons the view has shown recently, so a result that comes back while typing shows its icon
//! at once instead of a placeholder until the icon worker answers again.
use super::ApplicationIcon;
use std::{collections::HashMap, sync::Arc};

/// Several screens of results. Each entry keeps one icon handle alive.
pub(super) const CAPACITY: usize = 128;

/// The least recently used icon is evicted first. Rows share icons with the cache, so an
/// evicted icon is released only when no row still shows it.
#[derive(Default)]
pub(super) struct IconCache {
    /// Each icon and when it was last stored or used.
    entries: HashMap<Arc<str>, (Arc<ApplicationIcon>, u64)>,
    clock: u64,
}

impl IconCache {
    pub fn get(&mut self, identifier: &str) -> Option<Arc<ApplicationIcon>> {
        self.clock += 1;
        let (icon, used) = self.entries.get_mut(identifier)?;
        *used = self.clock;
        Some(icon.clone())
    }

    pub fn insert(&mut self, identifier: Arc<str>, icon: Arc<ApplicationIcon>) {
        self.clock += 1;
        if self.entries.len() >= CAPACITY && !self.entries.contains_key(&identifier) {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(identifier, _)| identifier.clone());
            if let Some(oldest) = oldest {
                self.entries.remove(&oldest);
            }
        }
        self.entries.insert(identifier, (icon, self.clock));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::{CopyIcon, LoadIconW, IDI_APPLICATION};

    #[test]
    fn the_least_recently_used_icon_is_evicted() {
        let icon = Arc::new(
            ApplicationIcon::from_handle(unsafe {
                CopyIcon(LoadIconW(None, IDI_APPLICATION).unwrap()).unwrap()
            })
            .unwrap(),
        );
        let mut cache = IconCache::default();
        for index in 0..CAPACITY {
            cache.insert(format!("app:{index}").into(), icon.clone());
        }
        // Showing the oldest again makes the second oldest the next to go.
        assert!(cache.get("app:0").is_some());
        cache.insert("app:new".into(), icon.clone());
        assert!(cache.get("app:1").is_none());
        assert!(cache.get("app:0").is_some());
        // Storing an icon that is already cached replaces it without evicting another.
        cache.insert("app:new".into(), icon.clone());
        assert_eq!(cache.entries.len(), CAPACITY);
        assert!(cache.get("app:2").is_some());
        assert!(cache.get("missing").is_none());
        drop(cache);
        assert_eq!(Arc::strong_count(&icon), 1);
    }
}
