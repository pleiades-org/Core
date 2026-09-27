//! European Central Bank reference rates for currency conversion. Work happens only when Core is
//! shown: the cached file loads first, and a background download refreshes it when stale.
use super::http;
use core_engine::conversions::ExchangeRates;
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        Globalization::{GetLocaleInfoEx, LOCALE_SINTLSYMBOL},
        UI::WindowsAndMessaging::{PostMessageW, WM_APP},
    },
};

pub const RATES_READY: u32 = WM_APP + 13;
const ECB_HOST: &str = "www.ecb.europa.eu";
const ECB_PATH: &str = "/stats/eurofxref/eurofxref-daily.xml";
const MAX_RATE_FILE_BYTES: usize = 64 * 1024;
/// The ECB publishes once per working day, around 16:00 CET.
const REFRESH_AFTER: Duration = Duration::from_secs(12 * 60 * 60);
/// After a failed download, wait before contacting the ECB again.
const RETRY_AFTER: Duration = Duration::from_secs(15 * 60);

#[derive(Clone)]
pub enum RateSource {
    /// Cached in AppData and refreshed from the ECB.
    Online(PathBuf),
    /// A fixed file (tests or offline use); never downloads.
    File(PathBuf),
    Disabled,
}

impl RateSource {
    pub fn online() -> Self {
        std::env::var_os("APPDATA").map_or(Self::Disabled, |root| {
            Self::Online(PathBuf::from(root).join("Pleiades/Core/v2/exchange-rates.xml"))
        })
    }
}

#[derive(Default)]
struct Shared {
    latest: Option<Arc<ExchangeRates>>,
    working: bool,
    loaded_cache: bool,
    last_download: Option<Instant>,
    shutdown: bool,
}

pub struct ExchangeRateService {
    source: RateSource,
    local_currency: Option<String>,
    shared: Arc<Mutex<Shared>>,
}

impl ExchangeRateService {
    pub fn new(source: RateSource) -> Self {
        Self {
            source,
            local_currency: local_currency(),
            shared: Arc::default(),
        }
    }

    /// Call when Core is shown. Returns immediately; `RATES_READY` is posted if rates change.
    pub fn refresh(&self, window: HWND) {
        let path = match &self.source {
            RateSource::Online(path) | RateSource::File(path) => path.clone(),
            RateSource::Disabled => return,
        };
        let download = matches!(self.source, RateSource::Online(_));
        {
            let mut shared = self.shared.lock().expect("exchange rate lock");
            let stale = download
                && cache_age(&path).is_none_or(|age| age >= REFRESH_AFTER)
                && shared
                    .last_download
                    .is_none_or(|attempt| attempt.elapsed() >= RETRY_AFTER);
            if shared.working || (shared.loaded_cache && !stale) {
                return;
            }
            shared.working = true;
            if stale {
                shared.last_download = Some(Instant::now());
            }
        }
        let shared = self.shared.clone();
        let local = self.local_currency.clone();
        let address = window.0 as usize;
        let spawned = std::thread::Builder::new()
            .name("core-exchange-rates".into())
            .spawn(move || {
                let publish = |rates: ExchangeRates| {
                    let rates = match &local {
                        Some(code) => rates.with_local_currency(code),
                        None => rates,
                    };
                    let mut state = shared.lock().expect("exchange rate lock");
                    state.latest = Some(Arc::new(rates));
                    // Checked under the lock so no message follows the service's shutdown.
                    if !state.shutdown {
                        post_ready(address);
                    }
                };
                let loaded = shared.lock().expect("exchange rate lock").loaded_cache;
                if !loaded {
                    match read_rates(&path) {
                        Ok(rates) => publish(rates),
                        Err(error) if download => eprintln!("No cached exchange rates: {error}"),
                        Err(error) => eprintln!("Could not load exchange rates: {error}"),
                    }
                    shared.lock().expect("exchange rate lock").loaded_cache = true;
                }
                if download && cache_age(&path).is_none_or(|age| age >= REFRESH_AFTER) {
                    match download_rates(&path) {
                        Ok(rates) => publish(rates),
                        Err(error) => eprintln!("Could not download exchange rates: {error}"),
                    }
                }
                shared.lock().expect("exchange rate lock").working = false;
            });
        if let Err(error) = spawned {
            eprintln!("Could not start the exchange rate worker: {error}");
            self.shared.lock().expect("exchange rate lock").working = false;
        }
    }

    pub fn latest(&self) -> Option<Arc<ExchangeRates>> {
        self.shared
            .lock()
            .expect("exchange rate lock")
            .latest
            .clone()
    }
}

impl Drop for ExchangeRateService {
    fn drop(&mut self) {
        // The worker is detached: a slow download must not delay exit.
        self.shared.lock().expect("exchange rate lock").shutdown = true;
    }
}

fn post_ready(address: usize) {
    if let Err(error) = unsafe {
        PostMessageW(
            Some(HWND(address as *mut _)),
            RATES_READY,
            WPARAM(0),
            LPARAM(0),
        )
    } {
        eprintln!("Could not deliver exchange rates: {error}");
    }
}

fn cache_age(path: &PathBuf) -> Option<Duration> {
    Some(
        fs::metadata(path)
            .ok()?
            .modified()
            .ok()?
            .elapsed()
            .unwrap_or_default(),
    )
}

fn read_rates(path: &PathBuf) -> Result<ExchangeRates, String> {
    let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
    ExchangeRates::from_ecb_xml(&text).map_err(str::to_owned)
}

/// Downloads, validates and atomically replaces the cache file.
fn download_rates(path: &PathBuf) -> Result<ExchangeRates, String> {
    let response = http::get(http::Request {
        secure: true,
        host: ECB_HOST,
        port: 443,
        path: ECB_PATH,
        max_bytes: MAX_RATE_FILE_BYTES,
    })
    .map_err(|error| error.to_string())?;
    if response.status != http::STATUS_OK {
        return Err(format!("the ECB answered HTTP {}", response.status));
    }
    let text = String::from_utf8(response.body).map_err(|_| "the rate file is not UTF-8")?;
    let rates = ExchangeRates::from_ecb_xml(&text).map_err(str::to_owned)?;
    let folder = path.parent().ok_or("no cache folder")?;
    fs::create_dir_all(folder).map_err(|error| error.to_string())?;
    let temporary = path.with_extension("xml.tmp");
    fs::write(&temporary, &text)
        .and_then(|_| fs::rename(&temporary, path))
        .map_err(|error| format!("could not cache rates: {error}"))?;
    Ok(rates)
}

/// The user's currency from Windows regional settings, such as `GBP`.
fn local_currency() -> Option<String> {
    let mut buffer = [0_u16; 9];
    let length = unsafe { GetLocaleInfoEx(PCWSTR::null(), LOCALE_SINTLSYMBOL, Some(&mut buffer)) };
    let code = String::from_utf16(&buffer[..usize::try_from(length).ok()?.checked_sub(1)?]).ok()?;
    (code.len() == 3 && code.bytes().all(|byte| byte.is_ascii_uppercase())).then_some(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Contacts the ECB: run with `cargo test -- --ignored ecb_rates_download`.
    #[test]
    #[ignore = "requires internet access"]
    fn ecb_rates_download_parse_and_cache() {
        let folder = std::env::temp_dir().join(format!("core-rates-{}", std::process::id()));
        let path = folder.join("exchange-rates.xml");
        let rates = download_rates(&path).unwrap();
        assert!(rates.per_euro("USD").is_some_and(|rate| rate > 0.));
        assert!(rates.per_euro("GBP").is_some_and(|rate| rate > 0.));
        assert_eq!(read_rates(&path).unwrap(), rates);
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn the_local_currency_is_a_three_letter_code_when_available() {
        assert!(local_currency().is_none_or(|code| code.len() == 3));
    }
}
