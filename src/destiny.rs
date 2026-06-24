//! Destiny 2 integration for Core Launcher (@d2 scope).
//!
//! - On boot: version check + conditional manifest download (background).
//! - Slim local cache for fast weapon lookups.
//! - Search results with icons and season indicators.
//! - Detailed weapon view (perk slots, Clarity, favoriting + role remembrance).

use crate::command::{CommandAction, CommandCategory, CommandResult, FeatureAction};
use crate::search_text::take_top_scored;
use crate::paths::{d2_data_dir, d2_debug_log};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MANIFEST_META_FILE: &str = "manifest_meta.json";
const WEAPONS_CACHE_FILE: &str = "weapons_cache.json";
const FAVORITES_FILE: &str = "favorites.json";
const ICONS_DIR_NAME: &str = "d2_icons";

const BUNGIE_BASE: &str = "https://www.bungie.net";

/// Bump when the weapons cache format or parsing logic changes (forces a rebuild).
const WEAPONS_CACHE_SCHEMA: u32 = 12;

/// Bungie drop-shadow layer paired with `secondaryBackground` season strips (DIM: watermarkDropShadowPath).
const SEASON_BANNER_SHADOW_PATH: &str = "/img/destiny_content/items/watermark-layer.png";

const AMMO_PRIMARY_ICON: &str = "ammo-primary.png";
const AMMO_SPECIAL_ICON: &str = "ammo-special.png";
const AMMO_HEAVY_ICON: &str = "ammo-heavy.png";

const TRAIT_TO_ENHANCED_JSON: &str = include_str!("../data/trait-to-enhanced-trait.json");
const WATERMARK_TO_SEASON_JSON: &str = include_str!("../data/watermark-to-season.json");

/// Known season hashes (from DestinySeasonDefinition) mapped to display names.
/// Used as fallback when the downloaded manifest is missing a season entry or the
/// local weapons cache was built against an older manifest (prevents "Unknown Season"
/// or raw "Season 1234567890" labels). Includes up to Season 28 (current as of mid-2026).
/// Season 29 hash can be added here when it appears in the manifest.
static KNOWN_SEASON_NAMES: &[(u32, &str)] = &[
    (965757574, "Red War"),
    (2973407602, "Curse of Osiris"),
    (4033618594, "Resurgence"),
    (2026773320, "Season of the Outlaw"),
    (2236269318, "Season of the Forge"),
    (2891088360, "Season of the Drifter"),
    (4275747712, "Season of Opulence"),
    (1743682818, "Season of the Undying"),
    (1743682819, "Season of Dawn"),
    (2809059425, "Season of the Worthy"),
    (2809059424, "Season of Arrivals"),
    (2809059427, "Season of the Hunt"),
    (2809059426, "Season of the Chosen"),
    (2809059429, "Season of the Splicer"),
    (2809059428, "Season of the Lost"),
    (2809059431, "Season of the Risen"),
    (2809059430, "Season of the Haunted"),
    (2809059433, "Season of Plunder"),
    (2809059432, "Season of the Seraph"),
    (2758726568, "Season of Defiance"),
    (2758726569, "Season of the Deep"),
    (2758726570, "Season of the Witch"),
    (2758726571, "Season of the Wish"),
    (2758726572, "Episode: Echoes"),
    (2758726573, "Episode: Revenant"),
    (2758726574, "Episode: Heresy"),
    (2758726575, "Season: Reclamation"),
    (2758726560, "Monument of Triumph"),
];

static WATERMARK_TO_SEASON: OnceLock<HashMap<String, u8>> = OnceLock::new();

fn watermark_to_season_index() -> &'static HashMap<String, u8> {
    WATERMARK_TO_SEASON.get_or_init(|| {
        serde_json::from_str::<HashMap<String, u8>>(WATERMARK_TO_SEASON_JSON)
            .unwrap_or_default()
    })
}

fn season_number_from_watermark_path(path: &str) -> Option<u8> {
    watermark_to_season_index().get(path).copied()
}

/// Resolved season number for display and filters (uses cache field, then watermark fallback).
pub fn weapon_season_number(weapon: &DestinyWeapon) -> Option<u8> {
    if let Some(raw) = weapon.season_name.as_ref().filter(|value| !value.is_empty()) {
        if let Ok(number) = raw.parse::<u8>() {
            return Some(number);
        }
    }
    weapon
        .season_watermark_path
        .as_deref()
        .and_then(season_number_from_watermark_path)
}

/// Resolved season label for display and filters — season number as a string (e.g. "28").
pub fn weapon_season_label(weapon: &DestinyWeapon) -> String {
    weapon_season_number(weapon)
        .map(|number| number.to_string())
        .unwrap_or_else(|| "?".to_string())
}

static ENHANCED_PERK_MAP: OnceLock<HashMap<u32, u32>> = OnceLock::new();

const CLARITY_URL: &str =
    "https://raw.githubusercontent.com/Database-Clarity/Live-Clarity-Database/live/descriptions/clarity.json";
const CLARITY_CACHE_FILE: &str = "clarity.json";

const MANIFEST_COMPONENTS: &[&str] = &[
    "DestinyInventoryItemDefinition",
    "DestinyPlugSetDefinition",
    "DestinySocketTypeDefinition",
    "DestinySeasonDefinition",
    "DestinyDamageTypeDefinition",
    "DestinyInventoryItemConstantsDefinition",
    "DestinyIconDefinition",
];

static D2_CONFIGURED: AtomicBool = AtomicBool::new(false);

/// Returns true if a bungie_api_key was present when the app started
/// (and we attempted to load Destiny data).
pub fn d2_configured() -> bool {
    D2_CONFIGURED.load(Ordering::Relaxed)
}

/// Call this (or it is called internally) when we detect a key at startup.
pub fn mark_d2_configured() {
    D2_CONFIGURED.store(true, Ordering::Relaxed);
}

/// Warm Destiny caches on a background thread so the first `@d2` query does not hitch the UI.
pub fn preload_runtime_data(api_key: Option<String>) {
    let _ = ensure_bundled_ammo_icons_cached();
    let _ = get_weapons();
    let _ = load_favorites();
    update_manifest_if_needed(api_key);
}

static MANIFEST_PROGRESS: OnceLock<Mutex<Option<ManifestDownloadProgress>>> = OnceLock::new();

static CACHED_WEAPONS: OnceLock<Mutex<Option<Arc<D2Cache>>>> = OnceLock::new();
static FAVORITED_WEAPON_INDICES: OnceLock<Mutex<Vec<usize>>> = OnceLock::new();
static D2_SEARCH_CACHE: OnceLock<Mutex<Option<D2SearchCacheEntry>>> = OnceLock::new();
const D2_SEARCH_CACHE_TTL: Duration = Duration::from_millis(150);

struct D2SearchCacheEntry {
    query: String,
    results: Vec<CommandResult>,
    cached_at: Instant,
}

#[derive(Clone, Debug, Default)]
pub struct ManifestDownloadProgress {
    pub stage: String,
    pub percent: f32,
    pub message: String,
}

pub fn current_manifest_progress() -> Option<ManifestDownloadProgress> {
    let mutex = MANIFEST_PROGRESS.get_or_init(|| Mutex::new(None));
    mutex.lock().ok().and_then(|g| (*g).clone())
}

fn update_manifest_progress(stage: &str, percent: f32, message: &str) {
    let prog = ManifestDownloadProgress {
        stage: stage.to_string(),
        percent: percent.clamp(0.0, 1.0),
        message: message.to_string(),
    };
    let mutex = MANIFEST_PROGRESS.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = mutex.lock() {
        *guard = Some(prog.clone());
    }
    log(&format!("[destiny] {} ({:.0}%) {}", stage, percent * 100.0, message));
}

fn clear_manifest_progress() {
    let mutex = MANIFEST_PROGRESS.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = mutex.lock() {
        *guard = None;
    }
}

fn load_weapons_from_disk() -> D2Cache {
    let path = weapons_cache_path();
    match fs::read_to_string(&path) {
        Ok(text) => {
            if let Ok(cache) = serde_json::from_str::<D2Cache>(&text) {
                return cache;
            }
        }
        Err(_) => {}
    }
    D2Cache::default()
}

pub fn get_weapons() -> Arc<D2Cache> {
    let m = CACHED_WEAPONS.get_or_init(|| Mutex::new(None));
    let mut guard = match m.lock() {
        Ok(g) => g,
        Err(_) => return Arc::new(D2Cache::default()),
    };
    if guard.is_none() {
        let _ = ensure_bundled_ammo_icons_cached();
        let mut cache = load_weapons_from_disk();
        cache.build_indexes();
        rebuild_favorited_weapon_indices(&cache, &load_favorites());
        let arc = Arc::new(cache);
        *guard = Some(arc.clone());
        return arc;
    }
    guard.as_ref().unwrap().clone()
}

fn favorited_weapon_indices() -> &'static Mutex<Vec<usize>> {
    FAVORITED_WEAPON_INDICES.get_or_init(|| Mutex::new(Vec::new()))
}

fn rebuild_favorited_weapon_indices(cache: &D2Cache, favs: &D2Favorites) {
    let indices = cache
        .weapons
        .iter()
        .enumerate()
        .filter_map(|(index, weapon)| favs.get(weapon.hash).favorited.then_some(index))
        .collect::<Vec<_>>();
    if let Ok(mut guard) = favorited_weapon_indices().lock() {
        *guard = indices;
    }
}

fn invalidate_d2_search_cache() {
    if let Some(cache) = D2_SEARCH_CACHE.get() {
        if let Ok(mut guard) = cache.lock() {
            *guard = None;
        }
    }
}

fn d2_log_path() -> PathBuf {
    d2_debug_log()
}

fn log(msg: &str) {
    // Always try to show in the terminal (cargo run)
    eprintln!("{}", msg);

    // Also append to a persistent log file the user can open
    let path = d2_log_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    let line = format!("[{}] {}\n", timestamp, msg);
    let _ = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, line.as_bytes()));
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
struct ManifestMeta {
    version: String,
    last_checked: u64,
    downloaded_components: Vec<String>,
    #[serde(default)]
    cache_schema_version: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WeaponStat {
    pub name: String,
    pub value: i32,
}

/// Weapon stat hashes from DestinyInventoryItemDefinition, in typical in-game display order.
const WEAPON_STAT_DEFINITIONS: &[(u32, &str)] = &[
    (4284893193, "RPM"),
    (3614673599, "Blast Radius"),
    (2523465841, "Velocity"),
    (4043523819, "Impact"),
    (1240592695, "Range"),
    (155624089, "Stability"),
    (3871231066, "Handling"),
    (4188031367, "Reload"),
    (360359141, "Magazine"),
    (2715839340, "Recoil"),
    (1345609583, "AA"),
    (3555269308, "Zoom"),
    (447667954, "Airborne"),
    (2961396640, "Charge Time"),
    (1931675084, "Draw Time"),
];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DestinyWeapon {
    pub hash: u32,
    pub name: String,
    pub icon_path: Option<String>, // relative Bungie path, e.g. "/common/destiny2_content/icons/..."
    pub screenshot: Option<String>,
    pub item_type: u32,
    pub tier_type: u32,
    pub season_hash: Option<u32>,
    pub season_name: Option<String>,
    #[serde(default)]
    pub season_banner_path: Option<String>,
    #[serde(default)]
    pub season_icon_path: Option<String>,
    /// Season banner strip from the weapon icon's `secondaryBackground` (DestinyIconDefinition).
    #[serde(default)]
    pub season_banner_overlay_path: Option<String>,
    /// Drop-shadow layer under the season banner strip (`watermarkDropShadowPath`).
    #[serde(default)]
    pub season_banner_shadow_path: Option<String>,
    /// Small season icon overlay (from `displayVersionWatermarkIcons` / `iconWatermark`).
    #[serde(default)]
    pub season_watermark_path: Option<String>,
    pub damage_type: Option<String>,
    #[serde(default)]
    pub damage_type_icon_path: Option<String>,
    pub ammo_type: Option<String>,
    #[serde(default)]
    pub ammo_type_icon_path: Option<String>,
    pub archetype: Option<String>,
    #[serde(default)]
    pub stats: Vec<WeaponStat>,
    // Simplified perk columns for the detail view.
    pub perk_columns: Vec<PerkColumn>,
    // Flat list of perk names for fast search/filtering (populated from real data when available).
    #[serde(default)]
    pub perk_names: Vec<String>,
    #[serde(skip)]
    pub perk_names_lower: Vec<String>,
    #[serde(skip)]
    pub name_lower: String,
    #[serde(skip)]
    pub archetype_lower: String,
    #[serde(skip)]
    pub search_haystack_lower: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct PerkColumn {
    pub slot_name: String, // e.g. "Trait 1", "Trait 2", "Intrinsic", "Barrel", "Magazine"
    pub perks: Vec<Perk>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Perk {
    pub hash: u32,
    pub name: String,
    pub description: String,       // from definition or fallback
    pub clarity_description: Option<String>, // from Clarity community data
    pub icon_path: Option<String>,
    /// Precomputed tooltip body (Clarity preferred, then manifest description).
    #[serde(default)]
    pub tooltip_text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct WeaponFavorite {
    pub favorited: bool,
    /// User-assigned roles / tags, e.g. ["PvE", "Godroll", "Crafting Target", "PvP"]
    pub roles: Vec<String>,
    /// Optional freeform notes for this weapon
    pub notes: String,
    /// Future: remembered specific perk choices per role
    pub remembered_rolls: Vec<RememberedRoll>,
    /// Perks the user clicked to save on this weapon (one per socket column).
    #[serde(default)]
    pub saved_perk_hashes: Vec<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RememberedRoll {
    pub role: String,
    pub perk_hashes: Vec<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct D2Cache {
    pub weapons: Vec<DestinyWeapon>,
    /// hash -> weapon for fast lookup
    #[serde(skip)]
    pub by_hash: HashMap<u32, DestinyWeapon>,
}

impl D2Cache {
    fn build_indexes(&mut self) {
        for weapon in &mut self.weapons {
            weapon.name_lower = weapon.name.to_lowercase();
            weapon.archetype_lower = weapon.archetype.as_deref().unwrap_or("").to_lowercase();
            weapon.perk_names_lower = weapon
                .perk_names
                .iter()
                .map(|name| name.to_lowercase())
                .collect();
            weapon.search_haystack_lower = format!(
                "{} {} {}",
                weapon.name_lower,
                weapon.archetype_lower,
                weapon.perk_names_lower.join(" ")
            );
        }
        self.by_hash = self
            .weapons
            .iter()
            .map(|w| (w.hash, w.clone()))
            .collect();
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct D2Favorites {
    pub weapons: HashMap<u32, WeaponFavorite>,
}

impl D2Favorites {
    pub fn get(&self, hash: u32) -> WeaponFavorite {
        self.weapons.get(&hash).cloned().unwrap_or_default()
    }

    pub fn set_favorited(&mut self, hash: u32, favorited: bool) {
        let entry = self.weapons.entry(hash).or_default();
        entry.favorited = favorited;
    }

    pub fn add_role(&mut self, hash: u32, role: String) {
        let entry = self.weapons.entry(hash).or_default();
        if !entry.roles.iter().any(|r| r.eq_ignore_ascii_case(&role)) {
            entry.roles.push(role);
        }
    }

    pub fn remove_role(&mut self, hash: u32, role: &str) {
        if let Some(entry) = self.weapons.get_mut(&hash) {
            entry.roles.retain(|r| !r.eq_ignore_ascii_case(role));
        }
    }
}

fn icons_dir() -> PathBuf {
    let dir = d2_data_dir().join(ICONS_DIR_NAME);
    let _ = fs::create_dir_all(&dir);
    dir
}

fn manifest_meta_path() -> PathBuf {
    d2_data_dir().join(MANIFEST_META_FILE)
}

fn weapons_cache_path() -> PathBuf {
    d2_data_dir().join(WEAPONS_CACHE_FILE)
}

fn favorites_path() -> PathBuf {
    d2_data_dir().join(FAVORITES_FILE)
}

/// Load the processed weapons cache (fast path for @d2 searches).
pub fn load_weapons_cache() -> D2Cache {
    let path = weapons_cache_path();
    match fs::read_to_string(&path) {
        Ok(text) => {
            if let Ok(mut cache) = serde_json::from_str::<D2Cache>(&text) {
                cache.build_indexes();
                return cache;
            }
        }
        Err(_) => {}
    }
    // No cache yet (or corrupt). Return empty; real data arrives after manifest download + processing.
    D2Cache::default()
}

static CACHED_FAVORITES: OnceLock<Mutex<Option<Arc<D2Favorites>>>> = OnceLock::new();

fn load_favorites_from_disk() -> D2Favorites {
    let path = favorites_path();
    match fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => D2Favorites::default(),
    }
}

/// Load or create the favorites store (role remembrance + hearts).
pub fn load_favorites() -> Arc<D2Favorites> {
    let mutex = CACHED_FAVORITES.get_or_init(|| Mutex::new(None));
    let mut guard = match mutex.lock() {
        Ok(guard) => guard,
        Err(_) => return Arc::new(D2Favorites::default()),
    };
    if guard.is_none() {
        *guard = Some(Arc::new(load_favorites_from_disk()));
    }
    guard
        .as_ref()
        .cloned()
        .unwrap_or_else(|| Arc::new(D2Favorites::default()))
}

pub fn save_favorites(favs: &D2Favorites) -> std::io::Result<()> {
    let path = favorites_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(favs).unwrap_or_else(|_| "{}".to_string());
    fs::write(path, text)?;
    let mutex = CACHED_FAVORITES.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = mutex.lock() {
        *guard = Some(Arc::new(favs.clone()));
    }
    if let Some(cache) = CACHED_WEAPONS.get().and_then(|mutex| mutex.lock().ok().and_then(|g| g.clone())) {
        rebuild_favorited_weapon_indices(cache.as_ref(), favs);
    }
    invalidate_d2_search_cache();
    Ok(())
}

/// The main entry point called on boot (from a background thread).
/// Performs a cheap version check against Bungie and downloads/updates
/// manifest components if the version changed or we have no local cache.
pub fn update_manifest_if_needed(api_key: Option<String>) {
    let api_key = api_key.filter(|k| !k.trim().is_empty());
    if api_key.is_none() {
        // User has not configured a (non-empty) key yet. Do nothing silently.
        return;
    }
    mark_d2_configured();
    let api_key = api_key.unwrap().trim().to_string();
    log("[destiny] Downloading Manifest...");
    update_manifest_progress("Initializing", 0.0, "Checking manifest version");
    let log_path = d2_log_path();
    log(&format!("[destiny] Detailed logs are being written to: {}", log_path.display()));

    let meta_path = manifest_meta_path();
    let current_meta = read_manifest_meta(&meta_path);

    // Fetch current manifest version (small request)
    let manifest_info = match fetch_manifest_info(&api_key) {
        Ok(info) => info,
        Err(e) => {
            eprintln!("[destiny] Failed to fetch manifest info: {}", e);
            clear_manifest_progress();
            return;
        }
    };

    let needs_version_update = current_meta
        .as_ref()
        .map(|m| m.version != manifest_info.version)
        .unwrap_or(true);
    let needs_schema_rebuild = current_meta
        .as_ref()
        .map(|m| m.cache_schema_version < WEAPONS_CACHE_SCHEMA)
        .unwrap_or(true);
    let needs_update = needs_version_update || needs_schema_rebuild;

    let cache_exists = weapons_cache_path().exists();

    if !needs_update && cache_exists {
        log(&format!("[destiny] Manifest is up to date (version {}). Using existing cache.", manifest_info.version));
        // Warm the in-memory cache so @d2 searches are instant after boot.
        let _ = get_weapons();
        update_manifest_progress("Complete", 1.0, "Using cached manifest");
        return;
    }

    log(&format!(
        "[destiny] Manifest update required (stored={:?}, remote={}). Downloading components...",
        current_meta.as_ref().map(|m| &m.version),
        manifest_info.version
    ));

    if let Err(e) = download_and_process_manifest(&api_key, &manifest_info) {
        eprintln!("[destiny] Manifest download/processing failed: {}", e);
        clear_manifest_progress();
        return;
    }

    // Write new meta
    let new_meta = ManifestMeta {
        version: manifest_info.version.clone(),
        last_checked: now_unix(),
        downloaded_components: MANIFEST_COMPONENTS.iter().map(|c| (*c).to_string()).collect(),
        cache_schema_version: WEAPONS_CACHE_SCHEMA,
    };
    let _ = write_manifest_meta(&meta_path, &new_meta);

    log("[destiny] Manifest processed. Weapons cache updated.");
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn read_manifest_meta(path: &Path) -> Option<ManifestMeta> {
    fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
}

fn write_manifest_meta(path: &Path, meta: &ManifestMeta) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(meta)?;
    fs::write(path, text)
}

#[derive(Debug)]
struct BungieManifestInfo {
    version: String,
    // Relative paths for English components
    component_paths: HashMap<String, String>,
}

fn fetch_manifest_info(api_key: &str) -> Result<BungieManifestInfo, String> {
    let url = format!("{}/Platform/Destiny2/Manifest/", BUNGIE_BASE);
    update_manifest_progress("Fetching manifest metadata", 0.02, &format!("key len {}", api_key.len()));
    let resp = ureq::get(&url)
        .set("X-API-Key", api_key)
        .timeout(std::time::Duration::from_secs(15))
        .call()
        .map_err(|e| format!("HTTP error: {}", e))?;

    if resp.status() != 200 {
        let status = resp.status();
        // Try to read body for error details
        let body = resp.into_string().unwrap_or_default();
        log(&format!("[destiny] Manifest fetch failed with status {}: {}", status, body));
        return Err(format!("Bungie returned status {}: {}", status, body));
    }

    let body_text = resp
        .into_string()
        .map_err(|e| format!("Failed to read manifest body: {}", e))?;
    let body: serde_json::Value = serde_json::from_str(&body_text)
        .map_err(|e| format!("JSON parse error: {}", e))?;
    update_manifest_progress("Manifest index received", 0.08, "Version and component URLs obtained");

    let response = &body["Response"];
    let version = response["version"]
        .as_str()
        .unwrap_or("unknown")
        .to_string();

    let mut component_paths = HashMap::new();
    if let Some(en_paths) = response["jsonWorldComponentContentPaths"].get("en") {
        if let Some(obj) = en_paths.as_object() {
            for (key, val) in obj {
                if let Some(rel) = val.as_str() {
                    component_paths.insert(key.clone(), rel.to_string());
                }
            }
        }
    }

    Ok(BungieManifestInfo {
        version,
        component_paths,
    })
}

#[derive(Clone)]
struct PlugInfo {
    hash: u32,
    name: String,
    description: String,
    icon_path: Option<String>,
}

#[derive(Clone)]
struct SeasonInfo {
    name: String,
    banner_path: Option<String>,
    icon_path: Option<String>,
}

#[derive(Clone)]
struct DamageTypeInfo {
    name: String,
    icon_path: Option<String>,
}

/// Downloads the key manifest components and builds our slim weapons cache.
fn download_and_process_manifest(api_key: &str, info: &BungieManifestInfo) -> Result<(), String> {
    let dir = d2_data_dir();
    let raw_dir = dir.join("raw");
    fs::create_dir_all(&raw_dir).map_err(|e| e.to_string())?;

    update_manifest_progress(
        "Manifest index received",
        0.09,
        "Downloading item, plug set, and socket definitions",
    );

    let component_count = MANIFEST_COMPONENTS.len() as f32;
    let mut downloaded: HashMap<String, String> = HashMap::new();

    for (index, component_key) in MANIFEST_COMPONENTS.iter().enumerate() {
        let progress_base = 0.10 + (index as f32 / component_count) * 0.70;
        let progress_span = 0.70 / component_count;
        let body = download_manifest_component(
            api_key,
            component_key,
            info,
            &raw_dir,
            progress_base,
            progress_span,
        )?;
        downloaded.insert((*component_key).to_string(), body);
    }

    update_manifest_progress(
        "Parsing manifest data",
        0.80,
        "Downloading Clarity perk descriptions",
    );
    let clarity_index = download_clarity_database().unwrap_or_else(|e| {
        log(&format!("[destiny] Clarity download failed ({}), using cached copy if available", e));
        load_clarity_index()
    });

    update_manifest_progress(
        "Parsing manifest data",
        0.82,
        "Building weapon cache with socket columns and perk icons",
    );

    let items: HashMap<String, serde_json::Value> = serde_json::from_str(
        downloaded
            .get("DestinyInventoryItemDefinition")
            .ok_or_else(|| "Missing DestinyInventoryItemDefinition body".to_string())?,
    )
    .map_err(|e| format!("Failed to parse item defs: {}", e))?;

    let plug_sets: HashMap<String, serde_json::Value> = serde_json::from_str(
        downloaded
            .get("DestinyPlugSetDefinition")
            .ok_or_else(|| "Missing DestinyPlugSetDefinition body".to_string())?,
    )
    .map_err(|e| format!("Failed to parse plug sets: {}", e))?;

    let socket_types: HashMap<String, serde_json::Value> = serde_json::from_str(
        downloaded
            .get("DestinySocketTypeDefinition")
            .ok_or_else(|| "Missing DestinySocketTypeDefinition body".to_string())?,
    )
    .map_err(|e| format!("Failed to parse socket types: {}", e))?;

    let seasons: HashMap<String, serde_json::Value> = serde_json::from_str(
        downloaded
            .get("DestinySeasonDefinition")
            .ok_or_else(|| "Missing DestinySeasonDefinition body".to_string())?,
    )
    .map_err(|e| format!("Failed to parse season definitions: {}", e))?;

    let damage_types: HashMap<String, serde_json::Value> = serde_json::from_str(
        downloaded
            .get("DestinyDamageTypeDefinition")
            .ok_or_else(|| "Missing DestinyDamageTypeDefinition body".to_string())?,
    )
    .map_err(|e| format!("Failed to parse damage type definitions: {}", e))?;

    let item_constants: HashMap<String, serde_json::Value> = serde_json::from_str(
        downloaded
            .get("DestinyInventoryItemConstantsDefinition")
            .ok_or_else(|| "Missing DestinyInventoryItemConstantsDefinition body".to_string())?,
    )
    .map_err(|e| format!("Failed to parse inventory item constants: {}", e))?;

    let icons: HashMap<String, serde_json::Value> = serde_json::from_str(
        downloaded
            .get("DestinyIconDefinition")
            .ok_or_else(|| "Missing DestinyIconDefinition body".to_string())?,
    )
    .map_err(|e| format!("Failed to parse icon definitions: {}", e))?;

    let plug_index = build_plug_index(&items);
    let plug_set_index = build_plug_set_index(&plug_sets);
    let socket_type_index = build_socket_type_index(&socket_types);
    let season_index = build_season_index(&seasons);
    let damage_type_index = build_damage_type_index(&damage_types);
    let icon_secondary_background_index = build_icon_secondary_background_index(&icons);
    let season_banner_shadow_path = item_constants
        .values()
        .find_map(|constants| {
            constants
                .get("watermarkDropShadowPath")
                .and_then(|value| value.as_str())
                .filter(|path| !path.is_empty())
                .map(|path| path.to_string())
        })
        .unwrap_or_else(|| SEASON_BANNER_SHADOW_PATH.to_string());

    ensure_bundled_ammo_icons_cached()?;

    log(&format!(
        "[destiny] Parsed {} plugs, {} plug sets, {} socket types, {} seasons, {} damage types",
        plug_index.len(),
        plug_set_index.len(),
        socket_type_index.len(),
        season_index.len(),
        damage_type_index.len()
    ));

    let mut weapons = Vec::new();

    for item in items.values() {
        if item["itemType"].as_u64() != Some(3) {
            continue;
        }

        let hash = item["hash"].as_u64().unwrap_or(0) as u32;
        if hash == 0 {
            continue;
        }

        let name = item["displayProperties"]["name"]
            .as_str()
            .unwrap_or("Unknown")
            .to_string();

        let icon = item["displayProperties"]["icon"]
            .as_str()
            .map(|s| s.to_string());

        let season_hash = item["seasonHash"].as_u64().map(|v| v as u32);
        let season_watermark_path = weapon_season_watermark_path(item);
        let season_meta = season_hash.and_then(|hash| season_index.get(&hash));
        let season_number = season_watermark_path
            .as_deref()
            .and_then(season_number_from_watermark_path);
        let season_name = season_meta
            .map(|season| season.name.clone())
            .filter(|name| !name.is_empty())
            .or_else(|| season_number.map(|number| number.to_string()));
        let season_banner_path = season_meta.and_then(|season| season.banner_path.clone());
        let season_icon_path = season_meta.and_then(|season| season.icon_path.clone());
        let icon_hash = item["displayProperties"]["iconHash"]
            .as_u64()
            .map(|value| value as u32);
        let season_banner_overlay_path = icon_hash
            .and_then(|hash| icon_secondary_background_index.get(&hash).cloned());
        let season_banner_shadow_path = season_banner_overlay_path
            .as_ref()
            .map(|_| season_banner_shadow_path.clone());

        let damage_hash = item["defaultDamageTypeHash"].as_u64().map(|v| v as u32);
        let damage_meta = damage_hash.and_then(|hash| damage_type_index.get(&hash));
        let damage_type = damage_meta.map(|damage| damage.name.clone());
        let damage_type_icon_path = damage_meta.and_then(|damage| damage.icon_path.clone());

        let ammo_type = item["equippingBlock"]["ammoType"]
            .as_u64()
            .and_then(ammo_type_label);
        let ammo_type_icon_path = item["equippingBlock"]["ammoType"]
            .as_u64()
            .and_then(ammo_type_icon_filename)
            .map(str::to_string);

        let perk_columns = build_weapon_perk_columns(
            item,
            &plug_index,
            &plug_set_index,
            &socket_type_index,
            &clarity_index,
        );
        let perk_names = collect_perk_names(&perk_columns);

        let weapon = DestinyWeapon {
            hash,
            name,
            icon_path: icon,
            screenshot: item["screenshot"].as_str().map(|s| s.to_string()),
            item_type: 3,
            tier_type: item["inventory"]["tierType"].as_u64().unwrap_or(0) as u32,
            season_hash,
            season_name,
            season_banner_path,
            season_icon_path,
            season_banner_overlay_path,
            season_banner_shadow_path,
            season_watermark_path,
            damage_type,
            damage_type_icon_path,
            ammo_type,
            ammo_type_icon_path,
            archetype: item["itemTypeDisplayName"].as_str().map(|s| s.to_string()),
            stats: build_weapon_stats(item),
            perk_columns,
            perk_names,
            perk_names_lower: Vec::new(),
            name_lower: String::new(),
            archetype_lower: String::new(),
            search_haystack_lower: String::new(),
        };

        weapons.push(weapon);
    }

    weapons.sort_by(|a, b| a.name.cmp(&b.name));

    let cache = D2Cache {
        weapons,
        by_hash: HashMap::new(),
    };

    let cache_text = serde_json::to_string_pretty(&cache).map_err(|e| e.to_string())?;
    fs::write(weapons_cache_path(), cache_text).map_err(|e| e.to_string())?;

    {
        let mut loaded = cache.clone();
        loaded.build_indexes();
        rebuild_favorited_weapon_indices(&loaded, &load_favorites());
        let arc = Arc::new(loaded);
        let m = CACHED_WEAPONS.get_or_init(|| Mutex::new(None));
        if let Ok(mut g) = m.lock() {
            *g = Some(arc);
        }
        invalidate_d2_search_cache();
    }

    log(&format!(
        "[destiny] Slim weapons cache written with {} weapons (version {}).",
        cache.weapons.len(),
        info.version
    ));
    update_manifest_progress("Complete", 1.0, &format!("{} weapons ready", cache.weapons.len()));
    log("[destiny] Manifest processing complete. Future @d2 searches should now return results.");

    Ok(())
}

fn download_manifest_component(
    api_key: &str,
    component_key: &str,
    info: &BungieManifestInfo,
    raw_dir: &Path,
    progress_base: f32,
    progress_span: f32,
) -> Result<String, String> {
    let rel_path = info
        .component_paths
        .get(component_key)
        .ok_or_else(|| format!("{component_key} path not present in manifest"))?;

    let full_url = format!("{BUNGIE_BASE}{rel_path}");
    let dest_path = raw_dir.join(format!("{component_key}.json"));
    let timeout_secs = if component_key.contains("InventoryItem") {
        300
    } else {
        120
    };

    log(&format!("[destiny] Downloading {component_key}..."));
    update_manifest_progress(
        "Downloading definitions",
        progress_base,
        &format!("Requesting {component_key} from Bungie"),
    );

    let resp = ureq::get(&full_url)
        .set("X-API-Key", api_key)
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .call()
        .map_err(|e| format!("Failed to download {component_key}: {e}"))?;

    let content_length = resp.header("Content-Length").and_then(|s| s.parse::<u64>().ok());
    let mut reader = resp.into_reader();
    let mut body = Vec::new();
    let mut downloaded = 0u64;
    let mut buf = [0u8; 65536];
    let mut last_update = Instant::now();

    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("Read error during {component_key} download: {e}"))?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&buf[..n]);
        downloaded += n as u64;
        if last_update.elapsed().as_millis() > 150 {
            if let Some(total) = content_length {
                let frac = (downloaded as f32 / total as f32).min(1.0);
                update_manifest_progress(
                    "Downloading definitions",
                    progress_base + progress_span * frac,
                    &format!(
                        "{component_key}: {:.1} / {:.1} MB",
                        downloaded as f32 / 1_048_576.0,
                        total as f32 / 1_048_576.0
                    ),
                );
            } else {
                update_manifest_progress(
                    "Downloading definitions",
                    progress_base + progress_span * 0.5,
                    &format!(
                        "{component_key}: {:.1} MB received",
                        downloaded as f32 / 1_048_576.0
                    ),
                );
            }
            last_update = Instant::now();
        }
    }

    log(&format!("[destiny] Downloaded {downloaded} bytes for {component_key}"));
    fs::write(&dest_path, &body).map_err(|e| e.to_string())?;
    String::from_utf8(body).map_err(|e| format!("UTF-8 error in {component_key}: {e}"))
}

fn build_plug_index(items: &HashMap<String, serde_json::Value>) -> HashMap<u32, PlugInfo> {
    let mut index = HashMap::new();
    for (hash_str, item) in items {
        let Ok(hash) = hash_str.parse::<u32>() else {
            continue;
        };
        let name = item["displayProperties"]["name"]
            .as_str()
            .unwrap_or("")
            .trim();
        if name.is_empty() {
            continue;
        }
        let description = item["displayProperties"]["description"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let icon_path = item["displayProperties"]["icon"]
            .as_str()
            .map(|s| s.to_string());
        index.insert(
            hash,
            PlugInfo {
                hash,
                name: name.to_string(),
                description,
                icon_path,
            },
        );
    }
    index
}

fn build_plug_set_index(plug_sets: &HashMap<String, serde_json::Value>) -> HashMap<u32, Vec<u32>> {
    let mut index = HashMap::new();
    for (hash_str, plug_set) in plug_sets {
        let Ok(set_hash) = hash_str.parse::<u32>() else {
            continue;
        };
        let mut plugs = Vec::new();
        if let Some(items) = plug_set
            .get("reusablePlugItems")
            .and_then(|value| value.as_array())
        {
            for plug in items {
                if let Some(plug_hash) = plug.get("plugItemHash").and_then(|value| value.as_u64()) {
                    plugs.push(plug_hash as u32);
                }
            }
        }
        if plugs.is_empty() {
            continue;
        }
        plugs.sort_unstable();
        plugs.dedup();
        index.insert(set_hash, plugs);
    }
    index
}

fn build_weapon_stats(item: &serde_json::Value) -> Vec<WeaponStat> {
    let Some(stats_obj) = item
        .get("stats")
        .and_then(|stats| stats.get("stats"))
        .and_then(|stats| stats.as_object())
    else {
        return Vec::new();
    };

    WEAPON_STAT_DEFINITIONS
        .iter()
        .filter_map(|(hash, name)| {
            let key = hash.to_string();
            let value = stats_obj
                .get(&key)
                .and_then(|entry| entry.get("value"))
                .and_then(|value| value.as_i64())?;
            if value <= 0 {
                return None;
            }
            Some(WeaponStat {
                name: (*name).to_string(),
                value: value as i32,
            })
        })
        .collect()
}

fn build_damage_type_index(
    damage_types: &HashMap<String, serde_json::Value>,
) -> HashMap<u32, DamageTypeInfo> {
    let mut index = HashMap::new();
    for (hash_str, damage_type) in damage_types {
        let Ok(hash) = hash_str.parse::<u32>() else {
            continue;
        };
        let name = damage_type["displayProperties"]["name"]
            .as_str()
            .unwrap_or("")
            .trim()
            .to_string();
        let icon_path = damage_type["displayProperties"]["icon"]
            .as_str()
            .filter(|path| !path.is_empty())
            .map(|path| path.to_string());
        if name.is_empty() && icon_path.is_none() {
            continue;
        }
        index.insert(
            hash,
            DamageTypeInfo {
                name: if name.is_empty() {
                    "Unknown".to_string()
                } else {
                    name
                },
                icon_path,
            },
        );
    }
    index
}

fn ammo_type_label(ammo_type: u64) -> Option<String> {
    match ammo_type {
        1 => Some("Primary".to_string()),
        2 => Some("Special".to_string()),
        3 => Some("Heavy".to_string()),
        _ => None,
    }
}

fn ammo_type_icon_filename(ammo_type: u64) -> Option<&'static str> {
    match ammo_type {
        1 => Some(AMMO_PRIMARY_ICON),
        2 => Some(AMMO_SPECIAL_ICON),
        3 => Some(AMMO_HEAVY_ICON),
        _ => None,
    }
}

fn weapon_season_watermark_path(item: &serde_json::Value) -> Option<String> {
    let version = item["quality"]["currentVersion"]
        .as_u64()
        .unwrap_or(0) as usize;
    if let Some(icons) = item["quality"]["displayVersionWatermarkIcons"].as_array() {
        if let Some(path) = icons.get(version).and_then(|value| value.as_str()) {
            if !path.is_empty() {
                return Some(path.to_string());
            }
        }
        if let Some(path) = icons.first().and_then(|value| value.as_str()) {
            if !path.is_empty() {
                return Some(path.to_string());
            }
        }
    }

    for key in ["iconWatermark", "iconWatermarkShelved", "iconWatermarkFeatured"] {
        if let Some(path) = item.get(key).and_then(|value| value.as_str()) {
            if !path.is_empty() {
                return Some(path.to_string());
            }
        }
    }

    None
}

fn bundled_ammo_icon_bytes(filename: &str) -> Option<&'static [u8]> {
    match filename {
        AMMO_PRIMARY_ICON => Some(include_bytes!("../data/d2_icons/ammo-primary.png")),
        AMMO_SPECIAL_ICON => Some(include_bytes!("../data/d2_icons/ammo-special.png")),
        AMMO_HEAVY_ICON => Some(include_bytes!("../data/d2_icons/ammo-heavy.png")),
        _ => None,
    }
}

fn ensure_bundled_ammo_icons_cached() -> Result<(), String> {
    for filename in [AMMO_PRIMARY_ICON, AMMO_SPECIAL_ICON, AMMO_HEAVY_ICON] {
        let local = icons_dir().join(filename);
        if local.exists() {
            continue;
        }
        let bytes = bundled_ammo_icon_bytes(filename)
            .ok_or_else(|| format!("Missing bundled ammo icon: {filename}"))?;
        fs::write(&local, bytes).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn build_icon_secondary_background_index(
    icons: &HashMap<String, serde_json::Value>,
) -> HashMap<u32, String> {
    let mut index = HashMap::new();
    for (hash_str, icon) in icons {
        let Ok(hash) = hash_str.parse::<u32>() else {
            continue;
        };
        let Some(path) = icon
            .get("secondaryBackground")
            .and_then(|value| value.as_str())
            .filter(|path| !path.is_empty())
        else {
            continue;
        };
        index.insert(hash, path.to_string());
    }
    index
}

fn build_season_index(seasons: &HashMap<String, serde_json::Value>) -> HashMap<u32, SeasonInfo> {
    let mut index = HashMap::new();
    for (hash_str, season) in seasons {
        let Ok(hash) = hash_str.parse::<u32>() else {
            continue;
        };
        let name = season["displayProperties"]["name"]
            .as_str()
            .unwrap_or("")
            .trim()
            .to_string();
        if name.is_empty() {
            continue;
        }
        let banner_path = season
            .get("backgroundImagePath")
            .and_then(|value| value.as_str())
            .filter(|path| !path.is_empty())
            .map(|path| path.to_string());
        let icon_path = season["displayProperties"]["icon"]
            .as_str()
            .filter(|path| !path.is_empty())
            .map(|path| path.to_string());
        index.insert(
            hash,
            SeasonInfo {
                name,
                banner_path,
                icon_path,
            },
        );
    }
    // Seed any known seasons missing from (or with blank name in) the current manifest.
    // Ensures name resolution + consistent labels even for edge seasons (e.g. Season 1 / Red War).
    for &(hash, name) in KNOWN_SEASON_NAMES {
        if !index.contains_key(&hash) {
            index.insert(
                hash,
                SeasonInfo {
                    name: name.to_string(),
                    banner_path: None,
                    icon_path: None,
                },
            );
        }
    }
    index
}

fn build_socket_type_index(
    socket_types: &HashMap<String, serde_json::Value>,
) -> HashMap<u32, String> {
    let mut index = HashMap::new();
    for (hash_str, socket_type) in socket_types {
        let Ok(hash) = hash_str.parse::<u32>() else {
            continue;
        };
        if let Some(name) = socket_type["displayProperties"]["name"].as_str() {
            if !name.is_empty() {
                index.insert(hash, name.to_string());
            }
        }
    }
    index
}

/// Socket entry positions to hide (1-based index in the weapon's socketEntries list).
fn should_skip_socket_index(socket_index: usize) -> bool {
    let one_based = socket_index + 1;
    (6..=8).contains(&one_based) || (10..=13).contains(&one_based)
}

fn should_skip_socket_type(name: &str) -> bool {
    let lower = name.to_lowercase();
    [
        "shader",
        "ornament",
        "mod socket",
        "masterwork",
        "tracker",
        "cosmetic",
        "appearance",
        "emblem",
        "ghost",
        "projection",
        "flavor",
        "memorial",
        "transmat",
        "emote",
    ]
    .iter()
    .any(|skip| lower.contains(skip))
}

fn plug_set_hashes_from_entry(entry: &serde_json::Value) -> Vec<u32> {
    let mut sets = Vec::new();
    for key in ["randomizedPlugSetHash", "plugSetHash"] {
        if let Some(hash) = entry.get(key).and_then(|value| value.as_u64()) {
            sets.push(hash as u32);
        }
    }
    if let Some(values) = entry
        .get("randomizedPlugSetHashes")
        .and_then(|value| value.as_array())
    {
        for value in values {
            if let Some(hash) = value.as_u64() {
                sets.push(hash as u32);
            }
        }
    }
    sets
}

fn collect_plug_hashes_for_socket(
    entry: &serde_json::Value,
    plug_sets: &HashMap<u32, Vec<u32>>,
) -> Vec<u32> {
    let mut hashes = Vec::new();
    if let Some(hash) = entry
        .get("singleInitialItemHash")
        .and_then(|value| value.as_u64())
    {
        if hash > 0 {
            hashes.push(hash as u32);
        }
    }
    if let Some(items) = entry
        .get("reusablePlugItems")
        .and_then(|value| value.as_array())
    {
        for plug in items {
            if let Some(plug_hash) = plug.get("plugItemHash").and_then(|value| value.as_u64()) {
                hashes.push(plug_hash as u32);
            }
        }
    }
    for set_hash in plug_set_hashes_from_entry(entry) {
        if let Some(set_plugs) = plug_sets.get(&set_hash) {
            hashes.extend(set_plugs.iter().copied());
        }
    }
    hashes.sort_unstable();
    hashes.dedup();
    hashes
}

fn download_clarity_database() -> Result<HashMap<u32, String>, String> {
    log("[destiny] Downloading Clarity perk database...");
    let resp = ureq::get(CLARITY_URL)
        .timeout(std::time::Duration::from_secs(120))
        .call()
        .map_err(|e| format!("Clarity HTTP error: {e}"))?;

    let body = resp
        .into_string()
        .map_err(|e| format!("Failed to read Clarity body: {e}"))?;

    let path = d2_data_dir().join(CLARITY_CACHE_FILE);
    fs::write(&path, &body).map_err(|e| e.to_string())?;
    parse_clarity_database(&body)
}

fn load_clarity_index() -> HashMap<u32, String> {
    let path = d2_data_dir().join(CLARITY_CACHE_FILE);
    match fs::read_to_string(&path) {
        Ok(text) => parse_clarity_database(&text).unwrap_or_default(),
        Err(_) => HashMap::new(),
    }
}

fn parse_clarity_database(body: &str) -> Result<HashMap<u32, String>, String> {
    let entries: HashMap<String, serde_json::Value> =
        serde_json::from_str(body).map_err(|e| format!("Clarity JSON parse error: {e}"))?;

    let mut index = HashMap::new();
    for (key, entry) in entries {
        let hash = key
            .parse::<u32>()
            .ok()
            .or_else(|| entry.get("hash").and_then(|value| value.as_u64()).map(|v| v as u32));
        let Some(hash) = hash else {
            continue;
        };
        if let Some(text) = extract_clarity_description(&entry) {
            index.insert(hash, text);
        }
    }

    log(&format!("[destiny] Loaded {} Clarity perk descriptions", index.len()));
    Ok(index)
}

fn extract_clarity_description(entry: &serde_json::Value) -> Option<String> {
    let blocks = entry
        .get("descriptions")
        .and_then(|descriptions| descriptions.get("en"))
        .and_then(|en| en.as_array())?;

    let mut parts = Vec::new();
    for block in blocks {
        let Some(lines) = block.get("linesContent").and_then(|value| value.as_array()) else {
            continue;
        };
        let line: String = lines
            .iter()
            .filter_map(|segment| segment.get("text").and_then(|text| text.as_str()))
            .collect();
        if !line.is_empty() {
            parts.push(line);
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n"))
    }
}

fn build_perk_tooltip_text(description: &str, clarity_description: &Option<String>) -> String {
    if let Some(clarity) = clarity_description
        .as_ref()
        .filter(|text| !text.trim().is_empty())
    {
        return clarity.clone();
    }
    if !description.trim().is_empty() {
        return description.to_string();
    }
    "No description available.".to_string()
}

/// Tooltip body for a perk: prefers Clarity, then Bungie manifest text.
pub fn perk_tooltip_text(perk: &Perk) -> String {
    if !perk.tooltip_text.is_empty() {
        return perk.tooltip_text.clone();
    }
    build_perk_tooltip_text(&perk.description, &perk.clarity_description)
}

fn build_weapon_perk_columns(
    item: &serde_json::Value,
    plug_index: &HashMap<u32, PlugInfo>,
    plug_sets: &HashMap<u32, Vec<u32>>,
    socket_types: &HashMap<u32, String>,
    clarity_index: &HashMap<u32, String>,
) -> Vec<PerkColumn> {
    let Some(entries) = item
        .get("sockets")
        .and_then(|sockets| sockets.get("socketEntries"))
        .and_then(|entries| entries.as_array())
    else {
        return Vec::new();
    };

    let mut columns = Vec::new();
    let mut socket_type_to_column: HashMap<u32, usize> = HashMap::new();
    let mut unnamed_socket = 0u32;

    for (socket_index, entry) in entries.iter().enumerate() {
        if should_skip_socket_index(socket_index) {
            continue;
        }

        let socket_type_hash = entry
            .get("socketTypeHash")
            .and_then(|value| value.as_u64())
            .unwrap_or(0) as u32;
        let slot_name = socket_types.get(&socket_type_hash).cloned().unwrap_or_else(|| {
            unnamed_socket += 1;
            format!("Socket {unnamed_socket}")
        });

        if should_skip_socket_type(&slot_name) {
            continue;
        }

        let plug_hashes = collect_plug_hashes_for_socket(entry, plug_sets);
        if plug_hashes.is_empty() {
            continue;
        }

        let mut seen_perk_hashes = HashSet::new();
        let perks: Vec<Perk> = plug_hashes
            .iter()
            .filter_map(|hash| plug_index.get(hash))
            .filter(|plug| seen_perk_hashes.insert(plug.hash))
            .map(|plug| {
                let clarity_description = clarity_index.get(&plug.hash).cloned();
                Perk {
                    hash: plug.hash,
                    name: plug.name.clone(),
                    description: plug.description.clone(),
                    clarity_description: clarity_description.clone(),
                    icon_path: plug.icon_path.clone(),
                    tooltip_text: build_perk_tooltip_text(
                        &plug.description,
                        &clarity_description,
                    ),
                }
            })
            .collect();

        if perks.is_empty() {
            continue;
        }

        if let Some(&column_index) = socket_type_to_column.get(&socket_type_hash) {
            merge_perks_into_column(&mut columns[column_index], perks);
            continue;
        }

        socket_type_to_column.insert(socket_type_hash, columns.len());
        columns.push(PerkColumn { slot_name, perks });
    }

    normalize_perk_columns(dedupe_perk_columns(columns))
}

fn enhanced_perk_map() -> &'static HashMap<u32, u32> {
    ENHANCED_PERK_MAP.get_or_init(|| {
        let raw: HashMap<String, u32> =
            serde_json::from_str(TRAIT_TO_ENHANCED_JSON).unwrap_or_default();
        raw.into_iter()
            .filter_map(|(base, enhanced)| base.parse::<u32>().ok().map(|base| (base, enhanced)))
            .collect()
    })
}

/// Drop base perk variants when their enhanced counterpart is also present in the same socket list.
fn filter_superseded_perks(perks: Vec<Perk>) -> Vec<Perk> {
    let enhanced_map = enhanced_perk_map();
    let present: HashSet<u32> = perks.iter().map(|perk| perk.hash).collect();

    perks
        .into_iter()
        .filter(|perk| {
            enhanced_map
                .get(&perk.hash)
                .is_none_or(|enhanced_hash| !present.contains(enhanced_hash))
        })
        .collect()
}

fn normalize_perk_columns(mut columns: Vec<PerkColumn>) -> Vec<PerkColumn> {
    for column in &mut columns {
        column.perks = filter_superseded_perks(std::mem::take(&mut column.perks));
    }
    columns.retain(|column| !column.perks.is_empty());
    columns
}

fn merge_perks_into_column(column: &mut PerkColumn, perks: Vec<Perk>) {
    let mut seen_hashes: HashSet<u32> = column.perks.iter().map(|perk| perk.hash).collect();
    for perk in perks {
        if seen_hashes.insert(perk.hash) {
            column.perks.push(perk);
        }
    }
}

fn perk_column_signature(column: &PerkColumn) -> Vec<u32> {
    column.perks.iter().map(|perk| perk.hash).collect()
}

fn dedupe_perk_columns(columns: Vec<PerkColumn>) -> Vec<PerkColumn> {
    let mut merged: Vec<PerkColumn> = Vec::new();

    for column in columns {
        if let Some(existing) = merged
            .iter_mut()
            .find(|existing| existing.slot_name.eq_ignore_ascii_case(&column.slot_name))
        {
            merge_perks_into_column(existing, column.perks);
            continue;
        }

        let signature = perk_column_signature(&column);
        if merged
            .iter()
            .any(|existing| perk_column_signature(existing) == signature)
        {
            continue;
        }

        merged.push(column);
    }

    merged
}

fn collect_perk_names(columns: &[PerkColumn]) -> Vec<String> {
    let mut names = Vec::new();
    for column in columns {
        for perk in &column.perks {
            if !names
                .iter()
                .any(|existing: &String| existing.eq_ignore_ascii_case(&perk.name))
            {
                names.push(perk.name.clone());
            }
        }
    }
    names
}

/// Cached local path for a weapon's icon, if already downloaded.
pub fn weapon_icon_if_cached(weapon: &DestinyWeapon) -> Option<PathBuf> {
    weapon.icon_path.as_deref().and_then(icon_if_cached)
}

/// Cached local path for a weapon's season banner strip overlay, if already downloaded.
pub fn weapon_season_banner_if_cached(weapon: &DestinyWeapon) -> Option<PathBuf> {
    weapon
        .season_banner_overlay_path
        .as_deref()
        .and_then(icon_if_cached)
}

/// Cached local path for the season banner drop-shadow layer, if already downloaded.
pub fn weapon_season_banner_shadow_if_cached(weapon: &DestinyWeapon) -> Option<PathBuf> {
    weapon
        .season_banner_shadow_path
        .as_deref()
        .and_then(icon_if_cached)
        .or_else(|| icon_if_cached(SEASON_BANNER_SHADOW_PATH))
}

/// Cached local path for a weapon's small season watermark icon, if already downloaded.
pub fn weapon_season_watermark_if_cached(weapon: &DestinyWeapon) -> Option<PathBuf> {
    weapon
        .season_watermark_path
        .as_deref()
        .and_then(icon_if_cached)
        .or_else(|| {
            weapon
                .season_icon_path
                .as_deref()
                .and_then(icon_if_cached)
        })
}

/// Cached local path for a weapon's damage type icon, if already downloaded.
pub fn weapon_damage_icon_if_cached(weapon: &DestinyWeapon) -> Option<PathBuf> {
    weapon
        .damage_type_icon_path
        .as_deref()
        .and_then(icon_if_cached)
}

/// Cached local path for a weapon's ammo type icon (bundled destiny-icons assets).
pub fn weapon_ammo_icon_if_cached(weapon: &DestinyWeapon) -> Option<PathBuf> {
    let filename = weapon.ammo_type_icon_path.as_deref()?;
    let local = icons_dir().join(filename);
    local.exists().then_some(local)
}

/// Ensure bundled ammo icons exist in the local icon cache.
pub fn ensure_bundled_ammo_icons() {
    let _ = ensure_bundled_ammo_icons_cached();
}

/// Returns a locally cached icon path without triggering a network download.
pub fn icon_if_cached(relative_path: &str) -> Option<PathBuf> {
    if relative_path.is_empty() {
        return None;
    }

    let filename = relative_path
        .rsplit('/')
        .next()
        .unwrap_or("icon.png")
        .to_string();
    let local = icons_dir().join(filename);
    local.exists().then_some(local)
}

static PENDING_ICON_DOWNLOADS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn pending_icon_downloads() -> &'static Mutex<HashSet<String>> {
    PENDING_ICON_DOWNLOADS.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Queue a Bungie icon for background download. Never blocks the caller.
pub fn request_icon_download(relative_path: &str) -> bool {
    if relative_path.is_empty() || icon_if_cached(relative_path).is_some() {
        return false;
    }

    let filename = relative_path
        .rsplit('/')
        .next()
        .unwrap_or("icon.png")
        .to_string();

    let mut pending = pending_icon_downloads()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !pending.insert(filename.clone()) {
        return false;
    }

    let path = relative_path.to_string();
    drop(pending);

    std::thread::spawn(move || {
        let _ = ensure_icon_cached(&path);
        if let Ok(mut pending) = pending_icon_downloads().lock() {
            pending.remove(&filename);
        }
    });
    true
}

/// Prefetch weapon, season, damage, and perk icons for a detail view.
pub fn prefetch_weapon_icons(weapon_hash: u32) {
    let Some((weapon, _)) = get_weapon_detail(weapon_hash) else {
        return;
    };

    let mut paths = Vec::new();
    if let Some(path) = weapon.icon_path.as_deref() {
        paths.push(path.to_string());
    }
    if let Some(path) = weapon.season_banner_overlay_path.as_deref() {
        paths.push(path.to_string());
    }
    if let Some(path) = weapon.season_banner_shadow_path.as_deref() {
        paths.push(path.to_string());
    }
    if let Some(path) = weapon.season_watermark_path.as_deref() {
        paths.push(path.to_string());
    }
    if let Some(path) = weapon.damage_type_icon_path.as_deref() {
        paths.push(path.to_string());
    }
    for column in &weapon.perk_columns {
        for perk in &column.perks {
            if let Some(path) = perk.icon_path.as_deref() {
                paths.push(path.to_string());
            }
        }
    }

    std::thread::spawn(move || {
        for path in paths {
            request_icon_download(&path);
        }
    });
}

/// Download a Bungie icon (weapon or perk) to our local cache and return the local path.
/// Prefer `icon_if_cached` on the UI thread and `request_icon_download` for background fetches.
pub fn ensure_icon_cached(relative_path: &str) -> Option<PathBuf> {
    if let Some(local) = icon_if_cached(relative_path) {
        return Some(local);
    }
    if relative_path.is_empty() {
        return None;
    }

    let filename = relative_path
        .rsplit('/')
        .next()
        .unwrap_or("icon.png")
        .to_string();

    let local = icons_dir().join(&filename);
    if local.exists() {
        return Some(local);
    }

    let url = format!("{}{}", BUNGIE_BASE, relative_path);
    match ureq::get(&url)
        .timeout(std::time::Duration::from_secs(20))
        .call()
    {
        Ok(resp) => {
            // ureq 2.x: stream the body into bytes for binary icon data
            let mut reader = resp.into_reader();
            let mut bytes = Vec::new();
            if std::io::Read::read_to_end(&mut reader, &mut bytes).is_ok() && fs::write(&local, &bytes).is_ok() {
                return Some(local);
            }
        }
        Err(e) => {
            eprintln!("[destiny] Icon download failed for {}: {}", relative_path, e);
        }
    }
    None
}

/// Produce search results for the @d2 scope.
/// `query` is everything after "@d2 ".
pub fn search_d2(query: &str) -> Vec<CommandResult> {
    let trimmed = query.trim();
    let cache = D2_SEARCH_CACHE.get_or_init(|| Mutex::new(None));
    if let Ok(guard) = cache.lock() {
        if let Some(entry) = guard.as_ref() {
            if entry.query == trimmed && entry.cached_at.elapsed() < D2_SEARCH_CACHE_TTL {
                return entry.results.clone();
            }
        }
    }

    let results = search_d2_uncached(query);
    if let Ok(mut guard) = cache.lock() {
        *guard = Some(D2SearchCacheEntry {
            query: trimmed.to_string(),
            results: results.clone(),
            cached_at: Instant::now(),
        });
    }
    results
}

fn search_d2_uncached(query: &str) -> Vec<CommandResult> {
    if let Some(p) = current_manifest_progress() {
        if p.percent < 1.0 {
            let title = "Downloading Destiny Manifest".to_string();
            let subtitle = format!("{} - {:.0}% {}", p.stage, p.percent * 100.0, p.message);
            let mut r = CommandResult::informational(&title, &subtitle);
            r.category = CommandCategory::Destiny;
            return vec![r];
        }
    }

    let cache = get_weapons();
    let favs = load_favorites();

    if cache.as_ref().weapons.is_empty() {
        let detail = if d2_configured() {
            "A bungie_api_key was detected. The Destiny manifest is being downloaded/processed in the background (first run can take 30-120+ seconds). All [destiny] logs are being written to d2_debug.log in your config folder (same place as config.toml). Open that file to see progress or errors. Try `@d2` again in a bit or restart after it completes."
        } else {
            "No bungie_api_key was detected at startup (see the 'Using config file:' and 'Bungie API key configured:' lines in the console when you run `cargo run`). Put `bungie_api_key = \"your-key\"` in the reported config.toml (or create one in the project folder for `cargo run`), then restart."
        };
        return vec![CommandResult::informational(
            "No Destiny 2 data loaded yet",
            detail,
        )];
    }

    let trimmed = query.trim();
    let q_lower = trimmed.to_lowercase();

    if q_lower.is_empty() {
        // Show a helpful home state + some favorites
        let mut results = vec![CommandResult::informational(
            "Destiny 2 Weapons (@d2)",
            "Fuzzy + DIM syntax (perk:xxx etc). Detailed logs (including download progress) go to d2_debug.log next to your config.toml. Open it with Notepad to see what's happening.",
        )];

        let fav_indices = favorited_weapon_indices()
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default();
        let mut fav_weapons: Vec<_> = fav_indices
            .iter()
            .filter_map(|index| cache.weapons.get(*index))
            .take(8)
            .collect();
        fav_weapons.sort_by_key(|weapon| weapon.name_lower.clone());

        for weapon in fav_weapons {
            results.push(make_weapon_result(weapon, &favs, 70));
        }
        prefetch_weapon_icons_for_results(&results);
        return results;
    }

    // DIM-style: while typing perk:/is:/has:/etc., show only filter suggestions — no weapons.
    if should_show_filter_autocomplete_only(trimmed) {
        let (key, partial) = active_filter_context(trimmed);
        let suggestions = build_d2_filter_suggestions(&key, &partial, cache.as_ref());
        if suggestions.is_empty() {
            return vec![CommandResult::informational(
                "Filter autocomplete",
                &format!("Keep typing to complete {key}:… or pick a suggestion when one appears."),
            )];
        }
        return suggestions;
    }

    let parsed = parse_d2_query(trimmed);
    let free_words = parsed
        .free_text
        .split_whitespace()
        .collect::<Vec<&str>>();
    let first_word = free_words.first().copied();

    let weapon_results = take_top_scored(
        cache.as_ref().weapons.iter().filter(|weapon| {
            first_word.is_none_or(|word| weapon.search_haystack_lower.contains(word))
                && matches_weapon_filters(weapon, &parsed)
        }),
        |weapon| {
            let score = weapon_result_score(weapon, &parsed);
            (score > 0).then_some(score)
        },
        20,
        |weapon| weapon.name_lower.clone(),
    );

    let mut results: Vec<CommandResult> = weapon_results
        .into_iter()
        .map(|(_score, weapon)| make_weapon_result(weapon, &favs, 85))
        .collect();
    prefetch_weapon_icons_for_results(&results);

    if results.is_empty() {
        results.push(CommandResult::informational(
            "No weapons matched",
            &format!("Try a different term for '{}'. Use perk:xxx, is:legendary, has:lightweight, etc.", query),
        ));
    }

    results
}

const IS_FILTER_SUGGESTIONS: &[&str] = &[
    "adept",
    "autorifle",
    "bow",
    "craftable",
    "exotic",
    "fusion",
    "glaive",
    "handcannon",
    "legendary",
    "linear",
    "pulse",
    "rare",
    "scout",
    "shotgun",
    "sidearm",
    "sniper",
    "submachine",
    "trace",
    "weapon",
];

const HAS_FILTER_SUGGESTIONS: &[&str] = &[
    "adept",
    "aggressive",
    "craftable",
    "dupelower",
    "duplicate",
    "godroll",
    "lightweight",
    "preciseframe",
    "randomroll",
];

const FILTER_AUTOCOMPLETE_KEYS: &[&str] = &[
    "perk", "perks", "perk1", "perk2", "perk3", "perk4", "perkname", "is", "has", "season", "s",
    "name",
];

/// Very small parsed representation for DIM-style queries.
#[derive(Default)]
struct ParsedD2Query {
    free_text: String,
    perk_filters: Vec<String>,
    is_filters: Vec<String>,
    has_filters: Vec<String>,
    season_filters: Vec<String>,
    name_filters: Vec<String>,
}

/// Rudimentary parser that supports multi-word values after a key: until the next key: pattern.
fn parse_d2_query(raw: &str) -> ParsedD2Query {
    let mut parsed = ParsedD2Query::default();
    let lower = raw.to_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();

    let mut idx = 0usize;
    while idx < words.len() {
        let word = words[idx];
        if let Some(colon) = word.find(':') {
            let key = &word[..colon];
            let mut value = word[colon + 1..].to_string();

            idx += 1;
            // greedily consume following words into this value until we hit something that looks like "newkey:val"
            while idx < words.len() {
                let next = words[idx];
                if next.contains(':') {
                    // looks like start of another filter
                    let potential_key = next.split(':').next().unwrap_or("");
                    if !potential_key.is_empty() && potential_key.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
                        break;
                    }
                }
                value.push(' ');
                value.push_str(next);
                idx += 1;
            }

            let v = value.trim().to_string();
            if !v.is_empty() {
                match key {
                    "perk" | "perks" | "perk1" | "perk2" | "perk3" | "perk4" | "perkname" => {
                        parsed.perk_filters.push(v);
                    }
                    "is" => {
                        parsed.is_filters.push(v);
                    }
                    "has" => {
                        parsed.has_filters.push(v);
                    }
                    "season" | "s" => {
                        parsed.season_filters.push(v);
                    }
                    "name" => {
                        parsed.name_filters.push(v);
                    }
                    _ => {
                        // unknown key - treat value as free text too
                        if !parsed.free_text.is_empty() {
                            parsed.free_text.push(' ');
                        }
                        parsed.free_text.push_str(&v);
                    }
                }
            }
        } else {
            if !parsed.free_text.is_empty() {
                parsed.free_text.push(' ');
            }
            parsed.free_text.push_str(word);
            idx += 1;
        }
    }

    parsed
}

fn is_filter_autocomplete_key(key: &str) -> bool {
    FILTER_AUTOCOMPLETE_KEYS
        .iter()
        .any(|candidate| candidate == &key)
}

/// True when the user is still typing a filter token. Committed filters end with trailing space
/// (e.g. after picking a suggestion), matching DIM's autocomplete-then-apply flow.
fn should_show_filter_autocomplete_only(raw: &str) -> bool {
    if raw.is_empty() {
        return false;
    }
    if raw.ends_with(' ') || raw.ends_with('\t') {
        return false;
    }

    let trimmed = raw.trim_end();
    if detect_partial_filter(trimmed).is_some() {
        return true;
    }

    if active_trailing_filter(trimmed).is_some() {
        return true;
    }

    detect_typing_filter_key_at_end(trimmed).is_some()
}

fn active_filter_context(raw: &str) -> (String, String) {
    if let Some((key, partial)) = detect_partial_filter(raw) {
        return (key, partial);
    }
    if let Some((key, partial)) = active_trailing_filter(raw) {
        return (key, partial);
    }
    if let Some(key) = detect_typing_filter_key_at_end(raw) {
        return (key, String::new());
    }
    (String::new(), String::new())
}

fn active_trailing_filter(raw: &str) -> Option<(String, String)> {
    let lower = raw.to_lowercase();
    const KEY_PREFIXES: &[&str] = &[
        "perkname:", "perks:", "perk:", "has:", "is:", "season:", "name:", "s:",
    ];

    let mut best: Option<(usize, &str)> = None;
    for prefix in KEY_PREFIXES {
        if let Some(index) = lower.rfind(prefix) {
            if best.is_none() || index > best.unwrap().0 {
                best = Some((index, prefix));
            }
        }
    }

    let (index, prefix) = best?;
    let key = prefix.trim_end_matches(':').to_string();
    if !is_filter_autocomplete_key(&key) {
        return None;
    }
    let partial = raw[index + prefix.len()..].trim().to_lowercase();
    Some((key, partial))
}

fn detect_typing_filter_key_at_end(raw: &str) -> Option<String> {
    let last_token = raw
        .rsplit_once(|c: char| c.is_whitespace())
        .map(|(_, token)| token)
        .unwrap_or(raw);
    let lower = last_token.to_lowercase();
    if lower.is_empty() {
        return None;
    }

    for key in FILTER_AUTOCOMPLETE_KEYS {
        if lower == *key {
            return Some(key.to_string());
        }
        if key.starts_with(&lower) && lower.len() < key.len() {
            return Some(key.to_string());
        }
    }
    None
}

fn build_d2_filter_suggestions(key: &str, partial: &str, cache: &D2Cache) -> Vec<CommandResult> {
    let partial = partial.trim().to_lowercase();
    let mut results = Vec::new();
    let mut seen = HashSet::new();

    let matches_partial = |candidate: &str| {
        partial.is_empty() || candidate.to_lowercase().contains(&partial)
    };

    match key {
        "perk" | "perks" | "perk1" | "perk2" | "perk3" | "perk4" | "perkname" => {
            let filter_key = if key == "perkname" { "perkname" } else { "perk" };
            for weapon in &cache.weapons {
                for (index, perk_lower) in weapon.perk_names_lower.iter().enumerate() {
                    if !matches_partial(perk_lower) {
                        continue;
                    }
                    let perk_name = weapon
                        .perk_names
                        .get(index)
                        .cloned()
                        .unwrap_or_else(|| perk_lower.clone());
                    if seen.insert(perk_name.clone()) {
                        results.push(make_d2_suggestion_result(
                            format!("{filter_key}:{perk_name}"),
                            "Perk filter",
                        ));
                        if results.len() >= 8 {
                            return results;
                        }
                    }
                }
            }
        }
        "is" => {
            for option in IS_FILTER_SUGGESTIONS {
                if matches_partial(option) {
                    let full = format!("is:{option}");
                    if seen.insert(full.clone()) {
                        results.push(make_d2_suggestion_result(full, "is: filter"));
                        if results.len() >= 8 {
                            break;
                        }
                    }
                }
            }
        }
        "has" => {
            for option in HAS_FILTER_SUGGESTIONS {
                if matches_partial(option) {
                    let full = format!("has:{option}");
                    if seen.insert(full.clone()) {
                        results.push(make_d2_suggestion_result(full, "has: filter"));
                        if results.len() >= 8 {
                            break;
                        }
                    }
                }
            }
        }
        "season" | "s" => {
            for weapon in &cache.weapons {
                let season = weapon_season_label(weapon);
                if season == "?" || !matches_partial(&season) {
                    continue;
                }
                if seen.insert(season.clone()) {
                    results.push(make_d2_suggestion_result(
                        format!("season:{season}"),
                        "Season filter",
                    ));
                    if results.len() >= 8 {
                        break;
                    }
                }
            }
        }
        "name" => {
            for weapon in &cache.weapons {
                if !matches_partial(&weapon.name) {
                    continue;
                }
                if seen.insert(weapon.name.clone()) {
                    results.push(make_d2_suggestion_result(
                        format!("name:{}", weapon.name),
                        "Weapon name filter",
                    ));
                    if results.len() >= 8 {
                        break;
                    }
                }
            }
        }
        _ => {}
    }

    results
}

/// Detect if the user is typing a partial filter at the end (for suggestions / autocomplete).
/// Returns (key, partial_value_lower)
fn detect_partial_filter(raw: &str) -> Option<(String, String)> {
    let trimmed = raw.trim_end();
    if let Some(last_space) = trimmed.rfind(' ') {
        let last = &trimmed[last_space + 1..];
        if let Some(colon) = last.find(':') {
            let key = last[..colon].to_lowercase();
            let partial = last[colon + 1..].to_lowercase();
            if is_filter_autocomplete_key(&key) && (!partial.is_empty() || last.ends_with(':')) {
                return Some((key, partial));
            }
        }
    } else if let Some(colon) = trimmed.find(':') {
        let key = trimmed[..colon].to_lowercase();
        let partial = trimmed[colon + 1..].to_lowercase();
        if is_filter_autocomplete_key(&key) {
            return Some((key, partial));
        }
    }
    None
}

fn matches_weapon_filters(w: &DestinyWeapon, parsed: &ParsedD2Query) -> bool {
    // is: filters
    for f in &parsed.is_filters {
        let fl = f.to_lowercase();
        let tier_name = match w.tier_type {
            6 => "exotic",
            5 => "legendary",
            4 => "rare",
            _ => "",
        };
        let archetype_l = &w.archetype_lower;
        let name_l = &w.name_lower;
        let matches = match fl.as_str() {
            "exotic" => tier_name == "exotic",
            "legendary" => tier_name == "legendary",
            "rare" => tier_name == "rare",
            "weapon" | "weapons" => w.item_type == 3,
            "craftable" => {
                archetype_l.contains("craft")
                    || weapon_season_label(w).to_lowercase().contains("craft")
            }
            "adept" => archetype_l.contains("adept") || name_l.contains("adept"),
            "autorifle" | "auto" => archetype_l.contains("auto rifle"),
            "pulse" => archetype_l.contains("pulse rifle"),
            "scout" => archetype_l.contains("scout rifle"),
            "handcannon" | "hand" => archetype_l.contains("hand cannon"),
            "bow" => archetype_l.contains("bow"),
            "sniper" => archetype_l.contains("sniper rifle"),
            "fusion" => archetype_l.contains("fusion rifle"),
            "shotgun" => archetype_l.contains("shotgun"),
            "sidearm" => archetype_l.contains("sidearm"),
            "glaive" => archetype_l.contains("glaive"),
            "linear" => archetype_l.contains("linear fusion"),
            "trace" => archetype_l.contains("trace rifle"),
            "submachine" | "smg" => archetype_l.contains("submachine"),
            _ => true,
        };
        if !matches {
            return false;
        }
    }

    // season filters (simple contains on the season string or hash str)
    for f in &parsed.season_filters {
        let fl = f.to_lowercase();
        let season_str = weapon_season_label(w).to_lowercase();
        let hash_str = w.season_hash.map(|h| h.to_string()).unwrap_or_default();
        if !season_str.contains(&fl) && !hash_str.contains(&fl) {
            return false;
        }
    }

    for pf in &parsed.perk_filters {
        let has = w
            .perk_names_lower
            .iter()
            .any(|perk_name| perk_name_matches_filter(perk_name, pf));
        if !has {
            return false;
        }
    }

    for filter in &parsed.has_filters {
        if !matches_has_filter(w, filter) {
            return false;
        }
    }

    for filter in &parsed.name_filters {
        if !w.name_lower.contains(&filter.to_lowercase()) {
            return false;
        }
    }

    true
}

fn matches_has_filter(weapon: &DestinyWeapon, filter: &str) -> bool {
    let filter = filter.trim().to_lowercase();
    let archetype = &weapon.archetype_lower;
    let name = &weapon.name_lower;
    let season = weapon_season_label(weapon).to_lowercase();

    match filter.as_str() {
        "adept" => archetype.contains("adept") || name.contains("adept"),
        "craftable" => {
            archetype.contains("craft") || season.contains("craft") || name.contains("craft")
        }
        "lightweight" | "aggressive" | "preciseframe" => weapon
            .perk_names_lower
            .iter()
            .any(|perk| perk.contains(&filter)),
        "godroll" | "randomroll" | "duplicate" | "dupelower" => true,
        _ => true,
    }
}

fn perk_name_matches_filter(perk_name_lower: &str, filter: &str) -> bool {
    let filter_lower = filter.trim().to_lowercase();
    if filter_lower.is_empty() {
        return true;
    }
    if perk_name_lower.contains(&filter_lower) {
        return true;
    }

    filter_lower
        .split_whitespace()
        .all(|word| perk_name_lower.contains(word))
}

fn query_has_active_filters(parsed: &ParsedD2Query) -> bool {
    !parsed.perk_filters.is_empty()
        || !parsed.is_filters.is_empty()
        || !parsed.has_filters.is_empty()
        || !parsed.season_filters.is_empty()
        || !parsed.name_filters.is_empty()
}

fn weapon_result_score(w: &DestinyWeapon, parsed: &ParsedD2Query) -> u8 {
    if parsed.free_text.is_empty() {
        if query_has_active_filters(parsed) {
            return 70;
        }
        return 0;
    }

    let score = compute_match_score(w, parsed);
    if score >= 55 {
        score
    } else {
        0
    }
}

fn normalize_d2_search_text(text: &str) -> String {
    text.trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn subsequence_match(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let mut remaining = needle.chars();
    let mut next = remaining.next();
    for ch in haystack.chars() {
        if next.is_some_and(|expected| ch == expected) {
            next = remaining.next();
        }
    }
    next.is_none()
}

fn score_text_match_normalized(query: &str, text: &str) -> u8 {
    if query.is_empty() || text.is_empty() {
        return 0;
    }

    if text == query {
        return 100;
    }
    if text.starts_with(query) {
        return 94;
    }
    if text.contains(query) {
        return 86;
    }

    for word in text.split_whitespace() {
        if word.starts_with(query) {
            return 80;
        }
        if word.contains(query) {
            return 74;
        }
    }

    if query.len() >= 3 && subsequence_match(text, query) {
        return 58;
    }

    0
}

fn score_text_match(query: &str, text: &str) -> u8 {
    score_text_match_normalized(&normalize_d2_search_text(query), &normalize_d2_search_text(text))
}

/// Rank weapons for free-text @d2 queries. Higher is better.
fn compute_match_score(w: &DestinyWeapon, parsed: &ParsedD2Query) -> u8 {
    if parsed.free_text.is_empty() {
        return 50;
    }

    let query = normalize_d2_search_text(&parsed.free_text);
    let name_score = score_text_match_normalized(&query, &w.name_lower);
    let archetype_score = if w.archetype_lower.is_empty() {
        0
    } else {
        score_text_match_normalized(&query, &w.archetype_lower)
    };
    let mut perk_best = 0u8;
    for perk_name in &w.perk_names_lower {
        perk_best = perk_best.max(score_text_match_normalized(&query, perk_name));
    }

    let mut combined = name_score
        .max(archetype_score.saturating_sub(4))
        .max(perk_best.saturating_sub(2));

    let words: Vec<&str> = query.split_whitespace().collect();
    if words.len() > 1 {
        let all_words_match = words.iter().all(|word| {
            w.search_haystack_lower.contains(word)
                || w.name_lower
                    .split_whitespace()
                    .any(|name_word| name_word.starts_with(word))
        });
        if !all_words_match {
            return 0;
        }
        combined = combined.max(60);
    }

    combined
}

/// Suggestion result that, when selected, will commit the full filter text into the search input
/// (this is what creates the visual "block" / separated token so that "perk:repulsor brace" stays together).
fn make_d2_suggestion_result(full_filter: String, subtitle: &str) -> CommandResult {
    CommandResult::feature(
        full_filter.clone(),
        subtitle.to_string(),
        CommandCategory::Destiny,
        FeatureAction::CommitD2Suggestion {
            suggestion: full_filter,
        },
        100, // suggestions rank high
    )
}

fn make_weapon_result(
    weapon: &DestinyWeapon,
    favs: &D2Favorites,
    confidence: u8,
) -> CommandResult {
    let season = weapon_season_label(weapon);

    let fav = favs.get(weapon.hash);
    let heart = if fav.favorited { "♥ " } else { "" };
    let roles = if fav.roles.is_empty() {
        String::new()
    } else {
        format!(" [{}]", fav.roles.join(", "))
    };
    let saved = saved_perk_labels(weapon, &fav);
    let saved_suffix = if saved.is_empty() {
        String::new()
    } else {
        format!(" • {}", saved.join(", "))
    };

    let title = format!("{}{}", heart, weapon.name);
    let subtitle = format!(
        "{} • {}{}{}",
        season,
        weapon.archetype.as_deref().unwrap_or("Weapon"),
        roles,
        saved_suffix
    );

    let icon_path = queue_weapon_icon_for_result(weapon);

    let mut result = CommandResult::feature(
        title,
        subtitle,
        CommandCategory::Destiny,
        FeatureAction::OpenDestinyWeapon {
            weapon_hash: weapon.hash,
        },
        confidence,
    );
    result.icon_path = icon_path;
    result
}

fn queue_weapon_icon_for_result(weapon: &DestinyWeapon) -> Option<PathBuf> {
    if let Some(relative_path) = weapon.icon_path.as_deref() {
        request_icon_download(relative_path);
        return icon_if_cached(relative_path);
    }
    None
}

fn prefetch_weapon_icons_for_results(results: &[CommandResult]) {
    let cache = get_weapons();
    let mut queued = 0usize;
    for result in results {
        if result.category != CommandCategory::Destiny {
            continue;
        }
        let CommandAction::Feature(FeatureAction::OpenDestinyWeapon { weapon_hash }) =
            &result.action
        else {
            continue;
        };
        let Some(weapon) = cache.by_hash.get(weapon_hash) else {
            continue;
        };
        if let Some(relative_path) = weapon.icon_path.as_deref() {
            if request_icon_download(relative_path) {
                queued += 1;
            }
        }
        if queued >= 24 {
            break;
        }
    }
}

/// Refresh cached icon paths on Destiny search rows after background downloads finish.
pub fn refresh_result_icons(results: &mut [CommandResult]) -> bool {
    let cache = get_weapons();
    let mut changed = false;
    for result in results.iter_mut() {
        if result.category != CommandCategory::Destiny || result.icon_path.is_some() {
            continue;
        }
        let CommandAction::Feature(FeatureAction::OpenDestinyWeapon { weapon_hash }) =
            &result.action
        else {
            continue;
        };
        let Some(weapon) = cache.by_hash.get(weapon_hash) else {
            continue;
        };
        if let Some(path) = queue_weapon_icon_for_result(weapon) {
            result.icon_path = Some(path);
            changed = true;
        }
    }
    changed
}

/// Called when the user accepts a Destiny weapon result.
/// For now this is just a marker; the real handling (switching to detail panel) lives in gpui_app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenDestinyWeaponAction {
    pub weapon_hash: u32,
}

// We extend FeatureAction in command.rs (see integration notes in that file).
// For the action executor we will add a case that returns a special ExecutedAction or lets
// the LauncherView handle "OpenDestinyWeapon" specially so it can show the rich detail view.

/// Helper to get a weapon + its favorite data together.
pub fn get_weapon_detail(hash: u32) -> Option<(DestinyWeapon, WeaponFavorite)> {
    let cache = get_weapons();
    let favs = load_favorites();
    cache.as_ref()
        .by_hash
        .get(&hash)
        .cloned()
        .map(|w| (w, favs.get(hash)))
}

/// Save an updated favorite record (called from detail view actions).
pub fn update_favorite(hash: u32, update: impl FnOnce(&mut WeaponFavorite)) {
    let mut favs = (*load_favorites()).clone();
    let entry = favs.weapons.entry(hash).or_default();
    update(entry);
    let _ = save_favorites(&favs);
}

fn apply_saved_perk_toggle(
    favorite: &mut WeaponFavorite,
    column_perk_hashes: &[u32],
    perk_hash: u32,
) {
    if let Some(index) = favorite
        .saved_perk_hashes
        .iter()
        .position(|hash| *hash == perk_hash)
    {
        favorite.saved_perk_hashes.remove(index);
        return;
    }

    favorite
        .saved_perk_hashes
        .retain(|hash| !column_perk_hashes.contains(hash));
    favorite.saved_perk_hashes.push(perk_hash);
    favorite.favorited = true;
}

fn apply_saved_perk_multi_toggle(favorite: &mut WeaponFavorite, perk_hash: u32) {
    if let Some(index) = favorite
        .saved_perk_hashes
        .iter()
        .position(|hash| *hash == perk_hash)
    {
        favorite.saved_perk_hashes.remove(index);
        return;
    }

    favorite.saved_perk_hashes.push(perk_hash);
    favorite.favorited = true;
}

/// Toggle a saved perk on a weapon. Selecting a perk in a column replaces any other saved perk
/// from that same column. Saving any perk also marks the weapon as favorited.
pub fn toggle_saved_weapon_perk(weapon_hash: u32, column_perk_hashes: &[u32], perk_hash: u32) {
    update_favorite(weapon_hash, |favorite| {
        apply_saved_perk_toggle(favorite, column_perk_hashes, perk_hash);
    });
}

/// Toggle a saved perk without replacing other perks in the same column (multi-roll saving).
pub fn toggle_saved_weapon_perk_multi(weapon_hash: u32, perk_hash: u32) {
    update_favorite(weapon_hash, |favorite| {
        apply_saved_perk_multi_toggle(favorite, perk_hash);
    });
}

/// Active DIM-style filter tokens for the @d2 scope query (everything after `@d2 `).
pub fn d2_query_filter_pills(scope_query: &str) -> Vec<String> {
    let parsed = parse_d2_query(scope_query.trim());
    let mut pills = Vec::new();
    for value in &parsed.perk_filters {
        pills.push(format!("perk:{value}"));
    }
    for value in &parsed.is_filters {
        pills.push(format!("is:{value}"));
    }
    for value in &parsed.has_filters {
        pills.push(format!("has:{value}"));
    }
    for value in &parsed.season_filters {
        pills.push(format!("season:{value}"));
    }
    for value in &parsed.name_filters {
        pills.push(format!("name:{value}"));
    }
    let free_text = parsed.free_text.trim();
    if !free_text.is_empty() {
        pills.push(free_text.to_string());
    }
    pills
}

pub fn saved_perk_labels(weapon: &DestinyWeapon, favorite: &WeaponFavorite) -> Vec<String> {
    let mut labels = Vec::new();
    for hash in &favorite.saved_perk_hashes {
        let label = weapon
            .perk_columns
            .iter()
            .flat_map(|column| column.perks.iter())
            .find(|perk| perk.hash == *hash)
            .map(|perk| perk.name.clone())
            .unwrap_or_else(|| format!("#{hash}"));
        if !labels.iter().any(|existing| existing == &label) {
            labels.push(label);
        }
    }
    labels
}

// Public re-exports for convenience in gpui_app and command_router
pub use self::OpenDestinyWeaponAction as _OpenDestinyWeaponAction; // placeholder until FeatureAction is extended

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_weapon(name: &str) -> DestinyWeapon {
        DestinyWeapon {
            hash: 1,
            name: name.to_string(),
            icon_path: None,
            screenshot: None,
            item_type: 3,
            tier_type: 5,
            season_hash: None,
            season_name: None,
            season_banner_path: None,
            season_icon_path: None,
            season_banner_overlay_path: None,
            season_banner_shadow_path: None,
            season_watermark_path: None,
            damage_type: None,
            damage_type_icon_path: None,
            ammo_type: None,
            ammo_type_icon_path: None,
            archetype: Some("Pulse Rifle".to_string()),
            stats: Vec::new(),
            perk_columns: Vec::new(),
            perk_names: vec!["Foreboding".to_string()],
            perk_names_lower: vec!["foreboding".to_string()],
            name_lower: name.to_lowercase(),
            archetype_lower: "pulse rifle".to_string(),
            search_haystack_lower: format!("{} pulse rifle foreboding", name.to_lowercase()),
        }
    }

    #[test]
    fn score_text_match_supports_partial_prefixes() {
        assert!(score_text_match("foreb", "Foreboding") >= 80);
        assert!(score_text_match("fore", "Before the Rain") >= 74);
        assert_eq!(score_text_match("foreboding", "Foreboding"), 100);
    }

    #[test]
    fn score_text_match_does_not_match_unrelated_substrings() {
        assert_eq!(score_text_match("foreb", "Zealous Epiphany"), 0);
        assert_eq!(score_text_match("foreb", "Before the Rain"), 0);
    }

    #[test]
    fn icon_if_cached_reads_local_destiny_icon_files() {
        let local = icons_dir().join("unit-test-icon.png");
        let _ = fs::write(&local, b"x");
        assert_eq!(
            icon_if_cached("/common/destiny2_content/icons/unit-test-icon.png"),
            Some(local.clone())
        );
        let _ = fs::remove_file(local);
    }

    #[test]
    fn compute_match_score_finds_weapons_by_perk_name() {
        let weapon = sample_weapon("Some Pulse Rifle");
        let parsed = parse_d2_query("foreb");
        assert!(compute_match_score(&weapon, &parsed) >= 70);
    }

    #[test]
    fn should_skip_socket_type_ignores_cosmetic_sockets() {
        assert!(should_skip_socket_type("Shader"));
        assert!(should_skip_socket_type("Weapon Ornament"));
        assert!(!should_skip_socket_type("Trait Column 1"));
        assert!(!should_skip_socket_type("Barrel"));
    }

    #[test]
    fn should_skip_socket_index_hides_cosmetic_socket_positions() {
        assert!(!should_skip_socket_index(0));
        assert!(!should_skip_socket_index(4));
        assert!(should_skip_socket_index(5));
        assert!(should_skip_socket_index(7));
        assert!(!should_skip_socket_index(8));
        assert!(should_skip_socket_index(9));
        assert!(should_skip_socket_index(12));
        assert!(!should_skip_socket_index(13));
    }

    #[test]
    fn perk_name_matches_filter_supports_multi_word_queries() {
        assert!(perk_name_matches_filter(
            "chaos reshaped",
            "chaos reshaped"
        ));
        assert!(perk_name_matches_filter("chaos reshaped", "chaos"));
        assert!(!perk_name_matches_filter("incandescent", "chaos reshaped"));
    }

    #[test]
    fn filter_autocomplete_mode_while_typing_perk_filter() {
        assert!(should_show_filter_autocomplete_only("perk:chaos"));
        assert!(should_show_filter_autocomplete_only("perk:"));
        assert!(!should_show_filter_autocomplete_only("perk:chaos reshaped "));
    }

    #[test]
    fn filter_autocomplete_mode_while_typing_is_or_has() {
        assert!(should_show_filter_autocomplete_only("is:leg"));
        assert!(should_show_filter_autocomplete_only("has:light"));
        assert!(should_show_filter_autocomplete_only("is"));
        assert!(!should_show_filter_autocomplete_only("is:legendary "));
    }

    #[test]
    fn free_text_weapon_search_is_not_filter_autocomplete() {
        assert!(!should_show_filter_autocomplete_only("foreboding"));
        assert!(!should_show_filter_autocomplete_only("hand cannon"));
    }

    #[test]
    fn apply_saved_perk_toggle_replaces_other_perks_in_same_column() {
        let mut favorite = WeaponFavorite::default();
        apply_saved_perk_toggle(&mut favorite, &[1, 2], 1);
        apply_saved_perk_toggle(&mut favorite, &[1, 2], 2);

        assert!(favorite.favorited);
        assert_eq!(favorite.saved_perk_hashes, vec![2]);
    }

    #[test]
    fn apply_saved_perk_multi_toggle_keeps_multiple_perks_in_same_column() {
        let mut favorite = WeaponFavorite::default();
        apply_saved_perk_multi_toggle(&mut favorite, 1);
        apply_saved_perk_multi_toggle(&mut favorite, 2);

        assert!(favorite.favorited);
        assert_eq!(favorite.saved_perk_hashes, vec![1, 2]);
        apply_saved_perk_multi_toggle(&mut favorite, 1);
        assert_eq!(favorite.saved_perk_hashes, vec![2]);
    }

    #[test]
    fn d2_query_filter_pills_collects_active_filters() {
        let pills = d2_query_filter_pills("hand cannon is:legendary perk:rampage");
        assert!(pills.iter().any(|pill| pill.contains("hand cannon")));
        assert!(pills.iter().any(|pill| pill.starts_with("is:")));
        assert!(pills.iter().any(|pill| pill.starts_with("perk:")));
    }

    #[test]
    fn filter_superseded_perks_removes_base_when_enhanced_present() {
        let enhanced_map = enhanced_perk_map();
        let Some((&base_hash, &enhanced_hash)) = enhanced_map.iter().next() else {
            return;
        };

        let filtered = filter_superseded_perks(vec![
            Perk {
                hash: base_hash,
                name: "Base".to_string(),
                description: String::new(),
                clarity_description: None,
                icon_path: None,
                tooltip_text: String::new(),
            },
            Perk {
                hash: enhanced_hash,
                name: "Enhanced".to_string(),
                description: String::new(),
                clarity_description: None,
                icon_path: None,
                tooltip_text: String::new(),
            },
        ]);

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].hash, enhanced_hash);
    }

    #[test]
    fn filter_superseded_perks_keeps_base_when_enhanced_missing() {
        let enhanced_map = enhanced_perk_map();
        let Some((&base_hash, _)) = enhanced_map.iter().next() else {
            return;
        };

        let filtered = filter_superseded_perks(vec![Perk {
            hash: base_hash,
            name: "Base".to_string(),
            description: String::new(),
            clarity_description: None,
            icon_path: None,
            tooltip_text: String::new(),
        }]);

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].hash, base_hash);
    }

    #[test]
    fn dedupe_perk_columns_merges_duplicate_trait_slots() {
        let columns = dedupe_perk_columns(vec![
            PerkColumn {
                slot_name: "Trait 1".to_string(),
                perks: vec![Perk {
                    hash: 1,
                    name: "Perk A".to_string(),
                    description: String::new(),
                    clarity_description: None,
                    icon_path: None,
                    tooltip_text: String::new(),
                }],
            },
            PerkColumn {
                slot_name: "trait 1".to_string(),
                perks: vec![Perk {
                    hash: 2,
                    name: "Perk B".to_string(),
                    description: String::new(),
                    clarity_description: None,
                    icon_path: None,
                    tooltip_text: String::new(),
                }],
            },
        ]);

        assert_eq!(columns.len(), 1);
        assert_eq!(columns[0].perks.len(), 2);
    }

    #[test]
    fn ammo_type_icon_filename_maps_manifest_values() {
        assert_eq!(ammo_type_icon_filename(1), Some(AMMO_PRIMARY_ICON));
        assert_eq!(ammo_type_icon_filename(2), Some(AMMO_SPECIAL_ICON));
        assert_eq!(ammo_type_icon_filename(3), Some(AMMO_HEAVY_ICON));
        assert_eq!(ammo_type_icon_filename(9), None);
    }

    #[test]
    fn build_weapon_stats_extracts_manifest_values() {
        let item = serde_json::json!({
            "stats": {
                "stats": {
                    "4284893193": { "value": 600 },
                    "4043523819": { "value": 77 },
                    "1240592695": { "value": 0 }
                }
            }
        });

        let stats = build_weapon_stats(&item);
        assert_eq!(stats.len(), 2);
        assert_eq!(stats[0].name, "RPM");
        assert_eq!(stats[0].value, 600);
        assert_eq!(stats[1].name, "Impact");
        assert_eq!(stats[1].value, 77);
    }

    #[test]
    fn season_number_from_watermark_resolves_current_season() {
        let path = "/common/destiny2_content/icons/e78fd9419f99464816ac8f628bc3c4af.png";
        assert_eq!(season_number_from_watermark_path(path), Some(28));
    }

    #[test]
    fn weapon_season_label_falls_back_to_watermark_when_cache_name_missing() {
        let mut weapon = sample_weapon("Test Weapon");
        weapon.season_watermark_path =
            Some("/common/destiny2_content/icons/da5f961ef97b78293cc498978c10e178.png".to_string());
        assert_eq!(weapon_season_label(&weapon), "3");
    }

    #[test]
    fn weapon_season_watermark_path_prefers_versioned_icon() {
        let item = serde_json::json!({
            "quality": {
                "currentVersion": 1,
                "displayVersionWatermarkIcons": [
                    "/common/destiny2_content/icons/old.png",
                    "/common/destiny2_content/icons/new.png"
                ]
            }
        });
        assert_eq!(
            weapon_season_watermark_path(&item),
            Some("/common/destiny2_content/icons/new.png".to_string())
        );
    }

    #[test]
    fn dedupe_perk_columns_drops_identical_perk_sets() {
        let perk = Perk {
            hash: 9,
            name: "Duplicate".to_string(),
            description: String::new(),
            clarity_description: None,
            icon_path: None,
            tooltip_text: String::new(),
        };
        let columns = dedupe_perk_columns(vec![
            PerkColumn {
                slot_name: "Trait 1".to_string(),
                perks: vec![perk.clone()],
            },
            PerkColumn {
                slot_name: "Trait 2".to_string(),
                perks: vec![perk],
            },
        ]);

        assert_eq!(columns.len(), 1);
    }

    #[test]
    fn weapon_result_score_includes_filter_only_perk_queries() {
        let mut weapon = sample_weapon("Test Weapon");
        weapon.perk_names = vec!["Chaos Reshaped".to_string()];
        weapon.perk_names_lower = vec!["chaos reshaped".to_string()];
        let parsed = parse_d2_query("perk:chaos reshaped");
        assert!(matches_weapon_filters(&weapon, &parsed));
        assert_eq!(weapon_result_score(&weapon, &parsed), 70);
    }
}
