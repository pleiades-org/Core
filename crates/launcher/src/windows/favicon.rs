//! Website icons for quicklinks, fetched at most once per cache period on the icon worker.
//! Google's favicon service is asked first because it also finds icons declared only in a page's
//! HTML; the site's own `/favicon.ico` is the fallback. Private hosts (`localhost`, IP addresses,
//! intranet names) are never sent to Google. No cookies or credentials are sent anywhere.
use super::{application_icon::ApplicationIcon, http};
use std::{fs, ops::Range, path::PathBuf, time::Duration};
use windows::Win32::UI::WindowsAndMessaging::{CreateIconFromResourceEx, LR_DEFAULTCOLOR};

/// Keep icons for a month; retry sites without a usable icon after a day.
const CACHE_LIFETIME: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const MISSING_RETRY: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_ICON_BYTES: usize = 512 * 1024;
/// Preferred favicon edge; rows draw icons at 26 px scaled for the display.
const PREFERRED_SIZE: u32 = 48;
const ICON_RESOURCE_VERSION: u32 = 0x0003_0000;
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
const ICO_HEADER: &[u8] = &[0, 0, 1, 0];
const GOOGLE_HOST: &str = "www.google.com";
/// Google scales icons to this edge; rows draw at 26 px, so 64 stays sharp at 200% scaling.
const GOOGLE_SIZE: u32 = 64;
/// Bumped when the icon source changes, so old "no icon" entries do not hide new results.
const CACHE_VERSION: u32 = 2;
/// Name suffixes that only resolve inside a private network.
const PRIVATE_SUFFIXES: &[&str] = &[".local", ".lan", ".internal", ".home.arpa", ".localhost"];

/// `scheme://host[:port]` for an HTTP(S) quicklink, lowercase. `None` for files and folders.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebsiteOrigin {
    secure: bool,
    host: String,
    port: u16,
}

impl WebsiteOrigin {
    pub fn parse(target: &str) -> Option<Self> {
        let (secure, rest) = if let Some(rest) = strip_prefix_ignore_case(target, "https://") {
            (true, rest)
        } else {
            (false, strip_prefix_ignore_case(target, "http://")?)
        };
        let authority = rest.split(['/', '?', '#']).next()?;
        // Credentials in a URL would be sent to the site; never request those links.
        if authority.contains('@') {
            return None;
        }
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) => (host, port.parse().ok()?),
            None => (authority, if secure { 443 } else { 80 }),
        };
        let host = host.to_ascii_lowercase();
        let valid = !host.is_empty()
            && host.len() <= 253
            && !host.starts_with('.')
            && !host.ends_with('.')
            && !host.contains("..")
            && host
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'));
        valid.then_some(Self { secure, host, port })
    }

    /// A file name that cannot escape the cache folder: only `[a-z0-9.-_]`.
    fn cache_name(&self) -> String {
        format!(
            "{}_{}_{}.v{CACHE_VERSION}.icon",
            if self.secure { "https" } else { "http" },
            self.host,
            self.port
        )
    }

    /// Public DNS names may be looked up by Google; private names and addresses never are.
    fn is_public(&self) -> bool {
        let host = self.host.as_str();
        let address = host
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.');
        host.contains('.')
            && !address
            && host != "localhost"
            && !PRIVATE_SUFFIXES.iter().any(|suffix| host.ends_with(suffix))
    }
}

fn strip_prefix_ignore_case<'text>(text: &'text str, prefix: &str) -> Option<&'text str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

/// Returns the cached icon, fetching it first when the cache is missing or stale.
pub fn load(origin: &WebsiteOrigin) -> Option<ApplicationIcon> {
    let path = cache_folder()?.join(origin.cache_name());
    if let Some(bytes) = fresh_cache(&path) {
        return decode(&bytes);
    }
    let bytes = match fetch(origin) {
        Ok(bytes) => bytes,
        Err(error) => {
            // Offline or unreachable: not cached, so the icon worker retries shortly.
            eprintln!("Could not fetch the icon for {}: {error}", origin.host);
            return None;
        }
    };
    let icon = decode(&bytes);
    // A source answered. An empty file records "no usable icon" so it is not asked again soon.
    let cached = if icon.is_some() {
        bytes.as_slice()
    } else {
        &[]
    };
    if icon.is_none() {
        eprintln!("{} has no icon from Google or /favicon.ico", origin.host);
    }
    if let Err(error) = fs::create_dir_all(path.parent()?).and_then(|_| fs::write(&path, cached)) {
        eprintln!("Could not cache the icon for {}: {error}", origin.host);
    }
    icon
}

fn cache_folder() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|root| PathBuf::from(root).join("Pleiades/Core/v2/favicons"))
}

fn fresh_cache(path: &PathBuf) -> Option<Vec<u8>> {
    let metadata = fs::metadata(path).ok()?;
    let age = metadata
        .modified()
        .ok()?
        .elapsed()
        .unwrap_or(Duration::ZERO);
    let lifetime = if metadata.len() == 0 {
        MISSING_RETRY
    } else {
        CACHE_LIFETIME
    };
    (age < lifetime).then(|| fs::read(path).ok()).flatten()
}

/// Accepts PNG or ICO data only; everything else (HTML error pages, SVG) is rejected.
fn decode(bytes: &[u8]) -> Option<ApplicationIcon> {
    let image = if bytes.starts_with(PNG_SIGNATURE) {
        bytes
    } else {
        &bytes[best_icon_image(bytes, PREFERRED_SIZE)?]
    };
    let icon = unsafe {
        CreateIconFromResourceEx(image, true, ICON_RESOURCE_VERSION, 0, 0, LR_DEFAULTCOLOR)
    }
    .ok()?;
    ApplicationIcon::from_handle(icon)
}

/// Chooses the image in an .ico file closest to `preferred` pixels, favoring larger images
/// and higher color depth. Every offset is bounds-checked against the downloaded bytes.
fn best_icon_image(bytes: &[u8], preferred: u32) -> Option<Range<usize>> {
    if !bytes.starts_with(ICO_HEADER) {
        return None;
    }
    let count = usize::from(u16::from_le_bytes([*bytes.get(4)?, *bytes.get(5)?]));
    (0..count)
        .filter_map(|index| {
            let entry = bytes.get(6 + index * 16..22 + index * 16)?;
            let edge = if entry[0] == 0 {
                256
            } else {
                u32::from(entry[0])
            };
            let depth = u16::from_le_bytes([entry[6], entry[7]]);
            let size = u32::from_le_bytes(entry[8..12].try_into().ok()?) as usize;
            let offset = u32::from_le_bytes(entry[12..16].try_into().ok()?) as usize;
            let end = offset.checked_add(size)?;
            (size > 0 && end <= bytes.len()).then_some((edge, depth, offset..end))
        })
        .min_by_key(|(edge, depth, _)| {
            // Downscaling looks better than upscaling, so undersized images cost double.
            let distance = if *edge >= preferred {
                edge - preferred
            } else {
                (preferred - edge) * 2
            };
            (distance, u16::MAX - depth)
        })
        .map(|(_, _, range)| range)
}

/// Icon bytes from the first source that has one; empty when every reachable source answered
/// without an icon. `Err` only when no source could be reached, so offline retries stay short.
fn fetch(origin: &WebsiteOrigin) -> windows::core::Result<Vec<u8>> {
    let google_path = format!("/s2/favicons?domain={}&sz={GOOGLE_SIZE}", origin.host);
    let google = http::Request {
        secure: true,
        host: GOOGLE_HOST,
        port: 443,
        path: &google_path,
        max_bytes: MAX_ICON_BYTES,
    };
    let direct = http::Request {
        secure: origin.secure,
        host: &origin.host,
        port: origin.port,
        path: "/favicon.ico",
        max_bytes: MAX_ICON_BYTES,
    };
    let sources = origin
        .is_public()
        .then_some(google)
        .into_iter()
        .chain([direct]);
    let mut reached = false;
    let mut last_error = None;
    for request in sources {
        match http::get(request) {
            // Google answers unknown sites with 404 and a generic globe; the body is then empty.
            Ok(response) if is_icon_data(&response.body) => return Ok(response.body),
            Ok(_) => reached = true,
            Err(error) => last_error = Some(error),
        }
    }
    match last_error {
        Some(error) if !reached => Err(error),
        _ => Ok(Vec::new()),
    }
}

fn is_icon_data(bytes: &[u8]) -> bool {
    bytes.starts_with(PNG_SIGNATURE) || best_icon_image(bytes, PREFERRED_SIZE).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_come_only_from_plain_http_and_https_hosts() {
        let origin = WebsiteOrigin::parse("HTTPS://Docs.Rust-Lang.org/std?x#y").unwrap();
        assert_eq!(origin.cache_name(), "https_docs.rust-lang.org_443.v2.icon");
        assert_eq!(
            WebsiteOrigin::parse("http://localhost:8080/")
                .unwrap()
                .cache_name(),
            "http_localhost_8080.v2.icon"
        );
        for target in [
            r"C:\Users",
            "https://user:secret@example.com/",
            "https://exa mple.com",
            "https://../../evil",
            "https://.example.com",
            "https://example.com:99999/",
            "ftp://example.com",
            "https://",
        ] {
            assert_eq!(WebsiteOrigin::parse(target), None, "{target}");
        }
    }

    #[test]
    fn only_public_hosts_are_sent_to_google() {
        for public in [
            "https://youtube.com",
            "https://docs.rust-lang.org",
            "http://example.co.uk:8080",
        ] {
            assert!(
                WebsiteOrigin::parse(public).unwrap().is_public(),
                "{public}"
            );
        }
        for private in [
            "http://localhost:3000",
            "http://192.168.1.10",
            "http://nas",
            "http://printer.local",
            "https://wiki.corp.internal",
            "http://router.lan",
        ] {
            assert!(
                !WebsiteOrigin::parse(private).unwrap().is_public(),
                "{private}"
            );
        }
    }

    fn ico(entries: &[(u8, u16, u32, u32)], length: usize) -> Vec<u8> {
        let mut bytes = vec![0, 0, 1, 0];
        bytes.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for (edge, depth, size, offset) in entries {
            bytes.extend_from_slice(&[*edge, *edge, 0, 0, 1, 0]);
            bytes.extend_from_slice(&depth.to_le_bytes());
            bytes.extend_from_slice(&size.to_le_bytes());
            bytes.extend_from_slice(&offset.to_le_bytes());
        }
        bytes.resize(length, 0);
        bytes
    }

    #[test]
    fn the_closest_bounded_image_is_chosen_from_an_ico_file() {
        let file = ico(
            &[
                (16, 32, 10, 100),
                (48, 8, 10, 110),
                (48, 32, 10, 120),
                (0, 32, 10, 130),
            ],
            140,
        );
        assert_eq!(best_icon_image(&file, 48), Some(120..130));
        assert_eq!(best_icon_image(&file, 200), Some(130..140));
        // Entries pointing past the downloaded bytes are ignored, not read.
        let truncated = ico(&[(48, 32, 1_000, 100), (16, 32, 10, 100)], 120);
        assert_eq!(best_icon_image(&truncated, 48), Some(100..110));
        let overflow = ico(&[(48, 32, u32::MAX, u32::MAX)], 64);
        assert_eq!(best_icon_image(&overflow, 48), None);
        assert_eq!(best_icon_image(b"<html>not an icon</html>", 48), None);
    }

    /// Contacts real websites: run with `cargo test -- --ignored favicons_download`.
    #[test]
    #[ignore = "requires internet access"]
    fn favicons_download_and_decode_from_real_sites() {
        // Decoded icons are process-wide USER handles; keep them off other tests' counts.
        let _serial = crate::windows::GUI_RESOURCE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // YouTube and rust-lang.org declare icons that `/favicon.ico` alone misses.
        for site in [
            "https://github.com",
            "https://www.wikipedia.org",
            "https://youtube.com",
            "https://www.rust-lang.org",
        ] {
            let origin = WebsiteOrigin::parse(site).unwrap();
            let bytes = fetch(&origin).unwrap_or_else(|error| panic!("{site}: {error}"));
            assert!(
                decode(&bytes).is_some(),
                "{site} returned {} bytes",
                bytes.len()
            );
        }
        // Google answers but has no icon; the site itself does not resolve.
        let missing = WebsiteOrigin::parse("https://core-favicon-test.invalid").unwrap();
        assert!(fetch(&missing).unwrap().is_empty());
    }

    #[test]
    fn a_new_miss_is_cached_and_absent_files_are_fetched() {
        let folder = std::env::temp_dir().join(format!("core-favicon-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        let missing = folder.join("missing.icon");
        fs::write(&missing, b"").unwrap();
        assert_eq!(fresh_cache(&missing), Some(Vec::new()));
        assert_eq!(fresh_cache(&folder.join("absent.icon")), None);
        fs::remove_dir_all(&folder).unwrap();
    }
}
