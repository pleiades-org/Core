use super::{
    app_aliases::{self, ShortcutReader},
    recent_applications::ApplicationAliases,
};
use core_engine::applications::{Application, ApplicationCatalog};
use std::os::windows::ffi::OsStrExt;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

const MAX_DEPTH: usize = 8;
const MAX_APPLICATIONS: usize = 10_000;
const MAX_ENTRIES: usize = 20_000;
const MAX_WARNINGS: usize = 32;

pub struct Discovery {
    pub catalog: Arc<ApplicationCatalog>,
    pub targets: HashMap<Arc<str>, PathBuf>,
    /// How Windows names these apps in its usage record.
    pub aliases: ApplicationAliases,
    pub warnings: Vec<String>,
}

/// Read Start Menu targets and the registered AppsFolder. Never crawl user files or resolve icons here.
pub fn discover_applications(stopped: &AtomicBool) -> Discovery {
    let mut targets = HashMap::new();
    let mut aliases = ApplicationAliases::default();
    let mut applications = Vec::new();
    let mut warnings = Vec::new();
    let mut visited = HashSet::new();
    let mut remaining_entries = MAX_ENTRIES;
    let shortcuts = ShortcutReader::new();
    for variable in ["ProgramData", "APPDATA"] {
        let Some(base) = std::env::var_os(variable) else {
            continue;
        };
        // Shell icon/launch APIs require native separators even though filesystem reads accept '/'.
        let root = PathBuf::from(base).join(r"Microsoft\Windows\Start Menu\Programs");
        visit_directory(
            &root,
            stopped,
            &mut |path| {
                if targets.len() >= MAX_APPLICATIONS || !visited.insert(path.clone()) {
                    return;
                }
                let Some(name) = path.file_stem() else { return };
                let identifier = identity(&path);
                applications.push(Application {
                    id: identifier.clone(),
                    name: name.to_string_lossy().replace(['_', '-'], " ").into(),
                    description: path
                        .parent()
                        .unwrap_or(&root)
                        .to_string_lossy()
                        .into_owned()
                        .into(),
                    pinned: false,
                    launches: 0,
                    aliases: shortcuts
                        .as_ref()
                        .and_then(|reader| reader.program_alias(&path))
                        .map(|alias| Arc::from([alias]))
                        .unwrap_or_default(),
                });
                aliases.add_shortcut(&path, &identifier);
                targets.insert(identifier, path);
            },
            &mut warnings,
            &mut remaining_entries,
        );
    }
    let mut desktop = Vec::new();
    let packaged = super::packaged_applications::discover(
        stopped,
        &mut |application, path| {
            if targets.len() < MAX_APPLICATIONS && !targets.contains_key(&application.id) {
                if let Some(app_id) = application.id.strip_prefix("package:") {
                    aliases.add_packaged(app_id, &application.id);
                }
                targets.insert(application.id.clone(), path);
                applications.push(application);
            }
        },
        &mut |entry| desktop.push(entry),
    );
    for entry in &desktop {
        aliases.add_desktop(entry);
    }
    let execution_aliases = app_aliases::execution_aliases();
    for application in &mut applications {
        if let Some(names) = application
            .id
            .strip_prefix("package:")
            .and_then(|app_id| execution_aliases.get(app_id))
        {
            application.aliases = names.as_slice().into();
        }
    }
    if let Err(error) = packaged {
        record_warning(
            &mut warnings,
            format!("Could not finish discovering Windows apps: {error}"),
        );
    }
    Discovery {
        catalog: Arc::new(ApplicationCatalog::new(applications)),
        targets,
        aliases,
        warnings,
    }
}

fn visit_directory(
    root: &Path,
    stopped: &AtomicBool,
    add: &mut impl FnMut(PathBuf),
    warnings: &mut Vec<String>,
    remaining_entries: &mut usize,
) {
    let mut pending = vec![(root.to_path_buf(), 0)];
    while let Some((directory, depth)) = pending.pop() {
        if stopped.load(Ordering::Relaxed) {
            return;
        }
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && depth == 0 => continue,
            Err(error) => {
                record_warning(
                    warnings,
                    format!("Could not read {}: {error}", directory.display()),
                );
                continue;
            }
        };
        for entry in entries {
            if *remaining_entries == 0 {
                record_warning(
                    warnings,
                    "Discovery stopped at the 20,000-entry safety limit".into(),
                );
                return;
            }
            *remaining_entries -= 1;
            if stopped.load(Ordering::Relaxed) {
                return;
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    record_warning(warnings, error.to_string());
                    continue;
                }
            };
            match entry.file_type() {
                Ok(kind) if kind.is_dir() && depth < MAX_DEPTH => {
                    pending.push((entry.path(), depth + 1))
                }
                Ok(kind) if kind.is_file() && is_launch_target(&entry.path()) => add(entry.path()),
                Ok(_) => {}
                Err(error) => record_warning(
                    warnings,
                    format!("Could not inspect {}: {error}", entry.path().display()),
                ),
            }
        }
    }
}

fn record_warning(warnings: &mut Vec<String>, warning: String) {
    if warnings.len() < MAX_WARNINGS {
        warnings.push(warning);
    }
}

fn is_launch_target(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            ["lnk", "exe", "appref-ms", "url"]
                .iter()
                .any(|supported| extension.eq_ignore_ascii_case(supported))
        })
}

fn identity(path: &Path) -> Arc<str> {
    use std::fmt::Write;
    let mut identifier = String::from("app:");
    for unit in path.as_os_str().encode_wide() {
        write!(identifier, "{unit:04x}").expect("writing to String cannot fail");
    }
    identifier.into()
}
