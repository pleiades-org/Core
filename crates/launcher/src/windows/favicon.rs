//! Website icons for quicklinks, fetched at most once per cache period on the icon worker.
//! Google's favicon service is asked first because it also finds icons declared only in a page's
//! HTML; the site's own `/favicon.ico` is the fallback. Private hosts (`localhost`, IP addresses,
//! intranet names) are never sent to Google. No cookies or credentials are sent anywhere.
use super::{application_icon::ApplicationIcon, http};
use std::{
    collections::BTreeMap,
    fs,
    ops::Range,
    path::PathBuf,
    sync::{Mutex, MutexGuard},
    time::{Duration, Instant},
};
use windows::Win32::UI::WindowsAndMessaging::{CreateIconFromResourceEx, LR_DEFAULTCOLOR};

/// Keep icons for a month; retry sites without a usable icon after a day.
const CACHE_LIFETIME: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const MISSING_RETRY: Duration = Duration::from_secs(24 * 60 * 60);
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
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
/// Waits after consecutive failures to reach a host; the last wait repeats.
const UNREACHABLE_BACKOFF: [Duration; 3] = [
    Duration::from_secs(30),
    Duration::from_secs(5 * 60),
    Duration::from_secs(60 * 60),
];
/// While Google cannot be reached, sites are asked for `/favicon.ico` directly.
const GOOGLE_UNREACHABLE: Duration = Duration::from_secs(5 * 60);

/// Unreachable sources, kept outside the icon worker's cache so evictions do not forget them.
static BACKOFF: Mutex<Backoff> = Mutex::new(Backoff::new());

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
    if let Some(icon) = cached_icon(&path) {
        return icon;
    }
    if backoff().host_waiting(&origin.host, Instant::now()) {
        return None;
    }
    let bytes = match fetch(origin) {
        Ok(bytes) => bytes,
        Err(error) => {
            // Offline or unreachable: not cached, and retried after a growing wait.
            let wait = backoff().host_unreachable(&origin.host, Instant::now());
            eprintln!(
                "Could not fetch the icon for {}: {error}; retrying in {} s",
                origin.host,
                wait.as_secs()
            );
            return None;
        }
    };
    backoff().host_reached(&origin.host);
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
    let temporary = path.with_extension("icon.tmp");
    if let Err(error) = fs::create_dir_all(path.parent()?)
        .and_then(|_| fs::write(&temporary, cached))
        .and_then(|_| fs::rename(&temporary, &path))
    {
        eprintln!("Could not cache the icon for {}: {error}", origin.host);
    }
    icon
}

fn cached_icon(path: &PathBuf) -> Option<Option<ApplicationIcon>> {
    let bytes = fresh_cache(path)?;
    if bytes.is_empty() {
        return Some(None);
    }
    if let Some(icon) = decode(&bytes) {
        return Some(Some(icon));
    }
    if let Err(error) = fs::remove_file(path) {
        eprintln!(
            "Could not remove invalid icon cache {}: {error}",
            path.display()
        );
    }
    None
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
/// without an icon. `Err` when no source could be reached, or when Google was skipped because
/// it was recently unreachable, so a miss is not recorded for a day without asking Google.
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
    let skip_google = origin.is_public() && !backoff().google_available(Instant::now());
    let sources = (origin.is_public() && !skip_google)
        .then_some((true, google))
        .into_iter()
        .chain([(false, direct)]);
    let deadline = Instant::now() + FETCH_TIMEOUT;
    let mut reached = false;
    let mut last_error = None;
    for (is_google, request) in sources {
        match get_icon(request, deadline) {
            // Google answers unknown sites with 404 and a generic globe; the body is then empty.
            Ok(response) if is_icon_data(&response.body) => return Ok(response.body),
            Ok(_) => reached = true,
            Err(error) => {
                if is_google {
                    backoff().google_unreachable(Instant::now());
                }
                last_error = Some(error);
            }
        }
    }
    match last_error {
        Some(error) if !reached => Err(error),
        _ if skip_google => Err(windows::core::Error::new(
            windows::Win32::Foundation::E_FAIL,
            "Google is unreachable and the site has no /favicon.ico",
        )),
        _ => Ok(Vec::new()),
    }
}

fn backoff() -> MutexGuard<'static, Backoff> {
    BACKOFF.lock().expect("favicon backoff lock")
}

struct Unreachable {
    failures: usize,
    retry_at: Instant,
}

/// Exponential backoff per unreachable host, and a pause for Google while it is unreachable.
struct Backoff {
    hosts: BTreeMap<String, Unreachable>,
    google_retry_at: Option<Instant>,
}

impl Backoff {
    const fn new() -> Self {
        Self {
            hosts: BTreeMap::new(),
            google_retry_at: None,
        }
    }

    fn host_waiting(&self, host: &str, now: Instant) -> bool {
        self.hosts
            .get(host)
            .is_some_and(|unreachable| now < unreachable.retry_at)
    }

    /// Records another failure and returns how long the host now waits.
    fn host_unreachable(&mut self, host: &str, now: Instant) -> Duration {
        let unreachable = self.hosts.entry(host.to_owned()).or_insert(Unreachable {
            failures: 0,
            retry_at: now,
        });
        let wait = UNREACHABLE_BACKOFF[unreachable.failures.min(UNREACHABLE_BACKOFF.len() - 1)];
        unreachable.failures = unreachable.failures.saturating_add(1);
        unreachable.retry_at = now + wait;
        wait
    }

    fn host_reached(&mut self, host: &str) {
        self.hosts.remove(host);
    }

    fn google_available(&self, now: Instant) -> bool {
        self.google_retry_at.is_none_or(|retry_at| now >= retry_at)
    }

    fn google_unreachable(&mut self, now: Instant) {
        self.google_retry_at = Some(now + GOOGLE_UNREACHABLE);
    }
}

fn get_icon(
    request: http::Request<'_>,
    deadline: Instant,
) -> windows::core::Result<http::Response> {
    let response = http::get(request, deadline)?;
    let Some(location) = &response.location else {
        return Ok(response);
    };
    let (origin, path) = redirect_target(request, location).ok_or_else(|| {
        windows::core::Error::new(
            windows::Win32::Foundation::E_FAIL,
            "unsafe favicon redirect",
        )
    })?;
    http::get(
        http::Request {
            secure: origin.secure,
            host: &origin.host,
            port: origin.port,
            path: &path,
            max_bytes: request.max_bytes,
        },
        deadline,
    )
}

fn redirect_target(request: http::Request<'_>, location: &str) -> Option<(WebsiteOrigin, String)> {
    if location.is_empty()
        || location.chars().any(|character| {
            character.is_whitespace() || character.is_control() || character == '\\'
        })
    {
        return None;
    }
    let scheme = if request.secure { "https" } else { "http" };
    let url = if location.starts_with("//") {
        format!("{scheme}:{location}")
    } else if location.contains("://") {
        location.to_owned()
    } else {
        let base = format!("{scheme}://{}:{}", request.host, request.port);
        if location.starts_with('/') {
            format!("{base}{location}")
        } else {
            let directory = request
                .path
                .rsplit_once('/')
                .map_or("", |(directory, _)| directory);
            format!("{base}{directory}/{location}")
        }
    };
    let origin = WebsiteOrigin::parse(&url)?;
    if !origin.is_public() || (request.secure && !origin.secure) {
        return None;
    }
    let rest = url.split_once("://")?.1;
    let path = rest
        .find(['/', '?', '#'])
        .map_or("/", |start| &rest[start..]);
    let path = path.split('#').next()?;
    let path = if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("/{path}")
    };
    Some((origin, path))
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
    fn redirects_revalidate_hosts_and_prevent_https_downgrades() {
        let request = http::Request {
            secure: true,
            host: "example.com",
            port: 443,
            path: "/favicon.ico",
            max_bytes: MAX_ICON_BYTES,
        };
        for location in [
            "https://cdn.example.com/icon.ico",
            "//cdn.example.com/icon.ico",
            "/icon.ico",
            "icon.ico",
        ] {
            let (origin, path) = redirect_target(request, location).unwrap();
            assert!(origin.is_public());
            assert_eq!(path, "/icon.ico");
        }
        for location in [
            "http://example.com/icon.ico",
            "https://127.0.0.1/icon",
            "https://nas/icon",
            "https://router.lan/icon",
            "https://localhost/icon",
            "https://user@site.com/icon",
            "https://site.com\\@localhost/icon",
            "",
        ] {
            assert!(redirect_target(request, location).is_none(), "{location}");
        }
        let private = http::Request {
            host: "printer.local",
            ..request
        };
        assert!(redirect_target(private, "/icon.ico").is_none());
    }

    #[test]
    fn unreachable_hosts_back_off_from_30_seconds_to_an_hour() {
        let mut backoff = Backoff::new();
        let now = Instant::now();
        let minutes = |count: u64| Duration::from_secs(count * 60);
        assert!(!backoff.host_waiting("nas", now));
        assert_eq!(
            backoff.host_unreachable("nas", now),
            Duration::from_secs(30)
        );
        assert!(backoff.host_waiting("nas", now + Duration::from_secs(29)));
        assert!(!backoff.host_waiting("nas", now + Duration::from_secs(30)));
        assert!(!backoff.host_waiting("example.com", now));
        assert_eq!(backoff.host_unreachable("nas", now), minutes(5));
        assert!(backoff.host_waiting("nas", now + minutes(5) - Duration::from_secs(1)));
        assert_eq!(backoff.host_unreachable("nas", now), minutes(60));
        assert_eq!(backoff.host_unreachable("nas", now), minutes(60));
        assert!(backoff.host_waiting("nas", now + minutes(59)));
        assert!(!backoff.host_waiting("nas", now + minutes(60)));
        backoff.host_reached("nas");
        assert!(!backoff.host_waiting("nas", now));
        assert_eq!(
            backoff.host_unreachable("nas", now),
            Duration::from_secs(30)
        );
    }

    #[test]
    fn google_is_skipped_for_a_few_minutes_after_it_is_unreachable() {
        let mut backoff = Backoff::new();
        let now = Instant::now();
        assert!(backoff.google_available(now));
        backoff.google_unreachable(now);
        assert!(!backoff.google_available(now));
        assert!(!backoff.google_available(now + GOOGLE_UNREACHABLE - Duration::from_secs(1)));
        assert!(backoff.google_available(now + GOOGLE_UNREACHABLE));
    }

    #[test]
    fn a_new_miss_is_cached_and_absent_files_are_fetched() {
        let folder = std::env::temp_dir().join(format!("core-favicon-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        let missing = folder.join("missing.icon");
        fs::write(&missing, b"").unwrap();
        assert_eq!(fresh_cache(&missing), Some(Vec::new()));
        assert!(matches!(cached_icon(&missing), Some(None)));
        let corrupt = folder.join("corrupt.icon");
        fs::write(&corrupt, b"not an icon").unwrap();
        assert!(cached_icon(&corrupt).is_none());
        assert!(!corrupt.exists());
        assert_eq!(fresh_cache(&folder.join("absent.icon")), None);
        fs::remove_dir_all(&folder).unwrap();
    }
}
