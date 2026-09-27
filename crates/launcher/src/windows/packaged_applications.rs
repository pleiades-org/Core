use core_engine::applications::Application;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use windows::{
    core::PWSTR,
    Win32::{System::Com::*, UI::Shell::*},
};

/// Enumerate installed packaged apps once on the discovery worker, including apps without .lnk files.
/// Desktop apps in AppsFolder, by app ID (`MSEdge`, `{known folder}\\app.exe`) and name. Windows
/// records their use under that ID, while Core lists them by their Start Menu shortcut.
pub struct DesktopEntry {
    pub app_id: String,
    pub name: String,
}

enum AppsFolderEntry {
    Packaged(Application, PathBuf),
    Desktop(DesktopEntry),
}

pub fn discover(
    stopped: &AtomicBool,
    add: &mut impl FnMut(Application, PathBuf),
    desktop: &mut impl FnMut(DesktopEntry),
) -> windows::core::Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
    }
    let _apartment = Apartment;
    let folder: IShellItem =
        unsafe { SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None)? };
    let items: IEnumShellItems = unsafe { folder.BindToHandler(None, &BHID_EnumItems)? };
    let mut skipped = 0_usize;
    for _ in 0..10_000 {
        if stopped.load(Ordering::Relaxed) {
            break;
        }
        let mut next = [None];
        unsafe {
            items.Next(&mut next, None)?;
        }
        let Some(item) = next[0].take() else {
            break;
        };
        // One broken package must not hide every app enumerated after it.
        match apps_folder_entry(&item) {
            Ok(Some(AppsFolderEntry::Packaged(application, path))) => add(application, path),
            Ok(Some(AppsFolderEntry::Desktop(entry))) => desktop(entry),
            Ok(None) => {}
            Err(_) => skipped += 1,
        }
    }
    if skipped > 0 {
        eprintln!("Skipped {skipped} packaged apps that Windows could not describe");
    }
    Ok(())
}

fn apps_folder_entry(item: &IShellItem) -> windows::core::Result<Option<AppsFolderEntry>> {
    let identifier = owned_text(unsafe { item.GetDisplayName(SIGDN_PARENTRELATIVEPARSING)? })?;
    if identifier.chars().any(char::is_control) {
        return Ok(None);
    }
    let name = owned_text(unsafe { item.GetDisplayName(SIGDN_NORMALDISPLAY)? })?;
    if name.is_empty() {
        return Ok(None);
    }
    // AppsFolder also contains desktop entries already indexed from Start Menu shortcuts.
    if !identifier.contains('!') {
        return Ok(Some(AppsFolderEntry::Desktop(DesktopEntry {
            app_id: identifier,
            name,
        })));
    }
    let path = PathBuf::from(format!("shell:AppsFolder\\{identifier}"));
    Ok(Some(AppsFolderEntry::Packaged(
        Application {
            id: Arc::from(format!("package:{identifier}")),
            name: name.into(),
            description: "Windows app".into(),
            pinned: false,
            launches: 0,
            aliases: Default::default(),
        },
        path,
    )))
}

fn owned_text(text: PWSTR) -> windows::core::Result<String> {
    let result = unsafe { text.to_string() };
    unsafe {
        CoTaskMemFree(Some(text.0.cast()));
    }
    Ok(result?)
}
struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}
