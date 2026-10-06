//! What Core asks of Spotify: catalog search for songs, albums and artists, the person's own
//! playlists, and playing what they chose.
use super::{
    authorization::{Authorization, PLAYLIST_SCOPE, REDIRECT_URI},
    encoding::{url_encode, valid_token},
    token_store,
};
use crate::windows::{http, settings::SpotifySettings};
use core_engine::search::{CatalogKind, Collection, CollectionKind, PlayMode, Song, SongStatus};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use windows::{
    core::HSTRING,
    Data::Json::{JsonArray, JsonObject},
};

const API_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_RESPONSE_BYTES: usize = 512 * 1024;
/// A catalog search asks for this many rows, and no more of an answer are read.
const SEARCH_RESULTS: u32 = 10;
/// Spotify hands a library over in pages of at most this many playlists.
const PLAYLIST_PAGE: usize = 50;
/// A library is read as far as this many playlists.
const MAX_PLAYLISTS: usize = 500;
#[derive(Debug)]
pub struct ApiError {
    pub status: SongStatus,
    pub message: &'static str,
}
pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    pub(super) fn invalid() -> Self {
        Self {
            status: SongStatus::NetworkError,
            message: "Spotify returned an invalid response. Try again later.",
        }
    }
    pub(super) fn cancelled() -> Self {
        Self {
            status: SongStatus::NetworkError,
            message: "Spotify playback was cancelled.",
        }
    }
    fn network() -> Self {
        Self {
            status: SongStatus::NetworkError,
            message: "Could not reach Spotify. Check your connection and try again.",
        }
    }
}

struct Tokens {
    access: String,
    refresh: String,
    expires: Instant,
    /// What the connection may do, as Spotify lists it with each token; None when it did not.
    scopes: Option<String>,
}

pub struct Client {
    pub settings: SpotifySettings,
    path: PathBuf,
    tokens: Option<Tokens>,
    blocked_until: Option<Instant>,
    transport: Box<dyn Transport>,
}

trait Transport: Send {
    fn send(
        &mut self,
        host: &str,
        method: &'static str,
        path: &str,
        headers: &str,
        body: &[u8],
    ) -> ApiResult<http::Response>;
}

struct WinHttpTransport;
impl Transport for WinHttpTransport {
    fn send(
        &mut self,
        host: &str,
        method: &'static str,
        path: &str,
        headers: &str,
        body: &[u8],
    ) -> ApiResult<http::Response> {
        http::exchange(
            http::Exchange {
                endpoint: http::Request {
                    secure: true,
                    host,
                    port: 443,
                    path,
                    max_bytes: MAX_RESPONSE_BYTES,
                },
                method,
                headers,
                body,
            },
            Instant::now() + API_TIMEOUT,
        )
        .map_err(|_| ApiError::network())
    }
}

impl Client {
    pub fn new(settings: SpotifySettings, path: PathBuf) -> Self {
        Self {
            settings,
            path,
            tokens: None,
            blocked_until: None,
            transport: Box::new(WinHttpTransport),
        }
    }

    pub fn exchange_code(&mut self, authorization: Authorization) -> ApiResult<()> {
        let body = format!(
            "grant_type=authorization_code&code={}&redirect_uri={}&client_id={}&code_verifier={}",
            url_encode(&authorization.code),
            url_encode(REDIRECT_URI),
            self.settings.client_id,
            url_encode(&authorization.verifier)
        );
        self.receive_tokens(&body, None)
    }

    pub fn search(&mut self, query: &str) -> ApiResult<Vec<Song>> {
        let response = self.authenticated("GET", &search_path(CatalogKind::Songs, query), &[])?;
        parse_songs(&response.body).map_err(|_| ApiError::invalid())
    }

    /// The albums or artists `@album` and `@artist` look for.
    pub fn search_collections(
        &mut self,
        kind: CatalogKind,
        query: &str,
    ) -> ApiResult<Vec<Collection>> {
        let response = self.authenticated("GET", &search_path(kind, query), &[])?;
        parse_collections(&response.body, kind).map_err(|_| ApiError::invalid())
    }

    /// `album`: the album the song is on, inside which it is started.
    pub fn play(
        &mut self,
        uri: &str,
        album: Option<&str>,
        cancelled: &impl Fn() -> bool,
    ) -> ApiResult<super::playback::PlaybackTarget> {
        super::playback::play_song(
            self,
            uri,
            album,
            &std::env::var("COMPUTERNAME").unwrap_or_default(),
            super::playback::wait,
            cancelled,
        )
    }

    /// A playlist, an album or an artist, from its start.
    pub fn play_collection(
        &mut self,
        uri: &str,
        mode: PlayMode,
        cancelled: &impl Fn() -> bool,
    ) -> ApiResult<super::playback::PlaybackTarget> {
        super::playback::play_collection(
            self,
            uri,
            mode,
            &std::env::var("COMPUTERNAME").unwrap_or_default(),
            super::playback::wait,
            cancelled,
        )
    }

    /// The person's own playlists, in their library's order.
    pub fn playlists(&mut self, cancelled: &impl Fn() -> bool) -> ApiResult<Vec<Collection>> {
        // The token says what the connection may do, which tells a connection made before
        // Core asked for playlists apart from an account that simply has none.
        self.access_token()?;
        if !self.granted(PLAYLIST_SCOPE) {
            return Err(ApiError {
                status: SongStatus::PermissionRequired,
                message: "Click Connect Spotify again in Music settings to allow your playlists.",
            });
        }
        let mut playlists: Vec<Collection> = Vec::new();
        for offset in (0..MAX_PLAYLISTS).step_by(PLAYLIST_PAGE) {
            let path = format!("/v1/me/playlists?limit={PLAYLIST_PAGE}&offset={offset}");
            let response = self.authenticated_if_current("GET", &path, &[], cancelled)?;
            let page = parse_playlists(&response.body)?;
            for playlist in page.playlists {
                if !playlists.iter().any(|known| known.uri == playlist.uri) {
                    playlists.push(playlist);
                }
            }
            if !page.more {
                break;
            }
        }
        Ok(playlists)
    }

    /// A token that does not list what it may do is tried: Spotify then answers for itself.
    fn granted(&self, scope: &str) -> bool {
        self.tokens
            .as_ref()
            .and_then(|tokens| tokens.scopes.as_deref())
            .is_none_or(|granted| granted.split_whitespace().any(|name| name == scope))
    }

    pub fn disconnect(&mut self) -> Result<(), String> {
        self.tokens = None;
        token_store::remove(&self.path)
    }

    fn access_token(&mut self) -> ApiResult<String> {
        if let Some(tokens) = self
            .tokens
            .as_ref()
            .filter(|tokens| tokens.expires > Instant::now() + Duration::from_secs(30))
        {
            return Ok(tokens.access.clone());
        }
        let refresh = match &self.tokens {
            Some(tokens) => Some(tokens.refresh.clone()),
            None => {
                token_store::load(&self.path, &self.settings.client_id).map_err(|_| ApiError {
                    status: SongStatus::ConnectRequired,
                    message:
                        "The saved Spotify connection could not be read. Disconnect and reconnect.",
                })?
            }
        }
        .ok_or(ApiError {
            status: SongStatus::ConnectRequired,
            message: "Connect Spotify in Settings → Music → Spotify song search.",
        })?;
        let body = format!(
            "grant_type=refresh_token&refresh_token={}&client_id={}",
            url_encode(&refresh),
            self.settings.client_id
        );
        self.receive_tokens(&body, Some(refresh))?;
        Ok(self
            .tokens
            .as_ref()
            .ok_or_else(ApiError::invalid)?
            .access
            .clone())
    }

    fn receive_tokens(&mut self, body: &str, old_refresh: Option<String>) -> ApiResult<()> {
        let response = self.request(
            "accounts.spotify.com",
            "POST",
            "/api/token",
            "Content-Type: application/x-www-form-urlencoded\r\n",
            body.as_bytes(),
        )?;
        let parsed = parse_json(&response.body)?;
        let access = string(&parsed, "access_token")?;
        if !string(&parsed, "token_type")?.eq_ignore_ascii_case("Bearer") {
            return Err(ApiError::invalid());
        }
        let refresh = parsed
            .GetNamedString(&HSTRING::from("refresh_token"))
            .ok()
            .map(|text| text.to_string())
            .or(old_refresh)
            .ok_or_else(ApiError::invalid)?;
        let expires = parsed
            .GetNamedNumber(&HSTRING::from("expires_in"))
            .map_err(|_| ApiError::invalid())?;
        if !valid_token(&access)
            || !valid_token(&refresh)
            || !expires.is_finite()
            || !(1.0..=86400.0).contains(&expires)
        {
            return Err(ApiError::invalid());
        }
        token_store::save(&self.path, &self.settings.client_id, &refresh).map_err(|_| ApiError { status: SongStatus::ConnectRequired, message: "Could not save the Spotify connection on this PC. Check Core's settings folder." })?;
        // A refreshed token that does not repeat the list keeps the one it replaces.
        let scopes = parsed
            .GetNamedString(&HSTRING::from("scope"))
            .ok()
            .map(|text| text.to_string())
            .or_else(|| self.tokens.take().and_then(|tokens| tokens.scopes));
        self.tokens = Some(Tokens {
            access,
            refresh,
            expires: Instant::now() + Duration::from_secs_f64(expires),
            scopes,
        });
        Ok(())
    }

    pub(super) fn authenticated(
        &mut self,
        method: &'static str,
        path: &str,
        body: &[u8],
    ) -> ApiResult<http::Response> {
        self.authenticated_if_current(method, path, body, &|| false)
    }

    pub(super) fn authenticated_if_current(
        &mut self,
        method: &'static str,
        path: &str,
        body: &[u8],
        cancelled: &impl Fn() -> bool,
    ) -> ApiResult<http::Response> {
        for attempt in 0..2 {
            let access = self.access_token()?;
            if cancelled() {
                return Err(ApiError::cancelled());
            }
            let headers =
                format!("Authorization: Bearer {access}\r\nContent-Type: application/json\r\n");
            match self.request("api.spotify.com", method, path, &headers, body) {
                Err(error) if error.status == SongStatus::ConnectRequired && attempt == 0 => {
                    if let Some(tokens) = &mut self.tokens {
                        tokens.expires = Instant::now();
                    }
                }
                result => return result,
            }
        }
        Err(ApiError {
            status: SongStatus::ConnectRequired,
            message: "Reconnect Spotify in Music settings.",
        })
    }

    fn request(
        &mut self,
        host: &str,
        method: &'static str,
        path: &str,
        headers: &str,
        body: &[u8],
    ) -> ApiResult<http::Response> {
        if self
            .blocked_until
            .is_some_and(|deadline| deadline > Instant::now())
        {
            return Err(ApiError {
                status: SongStatus::RateLimited,
                message: "Spotify's request limit was reached. Try again later.",
            });
        }
        let response = self
            .transport
            .send(host, method, path, headers, body)
            .inspect_err(|error| {
                if path.split('?').next() == Some("/v1/me/player/play") {
                    eprintln!("Spotify playback request failed: {}", error.message);
                }
            })?;
        if path.split('?').next() == Some("/v1/me/player/play") {
            // Record only the response status, never bearer headers or request bodies.
            eprintln!("Spotify playback: HTTP {}", response.status);
        }
        if response.status == 429 {
            self.blocked_until = Some(
                Instant::now()
                    + Duration::from_secs(u64::from(response.retry_after.unwrap_or(60)).max(1)),
            );
        }
        response_error(response.status).map_or(Ok(response), Err)
    }
}

fn response_error(status: u32) -> Option<ApiError> {
    let (status, message) = match status {
        200..=299 => return None,
        400 | 401 => (SongStatus::ConnectRequired, "Spotify authorization expired or the app setup is invalid. Check the Client ID and reconnect."),
        403 => (SongStatus::AccessDenied, "Spotify refused access. Check Premium and add your account to your Spotify app's allowed users."),
        404 => (SongStatus::NetworkError, "Open Spotify and play a track on your chosen device once, then try again."),
        429 => (SongStatus::RateLimited, "Spotify's request limit was reached. Try again later."),
        _ => (SongStatus::NetworkError, "Spotify could not complete the request. Try again later."),
    };
    Some(ApiError { status, message })
}

pub(super) fn parse_json(bytes: &[u8]) -> ApiResult<JsonObject> {
    let text = std::str::from_utf8(bytes).map_err(|_| ApiError::invalid())?;
    JsonObject::Parse(&HSTRING::from(text)).map_err(|_| ApiError::invalid())
}

pub(super) fn string(object: &JsonObject, name: &str) -> ApiResult<String> {
    object
        .GetNamedString(&HSTRING::from(name))
        .map(|text| text.to_string())
        .map_err(|_| ApiError::invalid())
}

pub fn valid_track_uri(uri: &str) -> bool {
    valid_uri(uri, "spotify:track:")
}

pub fn valid_album_uri(uri: &str) -> bool {
    valid_uri(uri, "spotify:album:")
}

pub fn valid_playlist_uri(uri: &str) -> bool {
    valid_uri(uri, "spotify:playlist:")
}

pub fn valid_artist_uri(uri: &str) -> bool {
    valid_uri(uri, "spotify:artist:")
}

/// Anything Spotify plays as a whole: a playlist, an album or an artist.
pub fn valid_collection_uri(uri: &str) -> bool {
    valid_playlist_uri(uri) || valid_album_uri(uri) || valid_artist_uri(uri)
}

fn search_path(kind: CatalogKind, query: &str) -> String {
    format!(
        "/v1/search?type={}&limit={SEARCH_RESULTS}&q={}",
        kind.api_type(),
        url_encode(query)
    )
}

/// Spotify's addresses are a kind and 22 letters or digits. They are written into requests,
/// so nothing else may pass.
fn valid_uri(uri: &str, kind: &str) -> bool {
    uri.strip_prefix(kind)
        .is_some_and(|id| id.len() == 22 && id.bytes().all(|byte| byte.is_ascii_alphanumeric()))
}

pub(super) fn bounded(text: String) -> Arc<str> {
    text.chars()
        .filter(|character| !character.is_control())
        .take(256)
        .collect::<String>()
        .into()
}

fn parse_songs(bytes: &[u8]) -> ApiResult<Vec<Song>> {
    let root = parse_json(bytes)?;
    let items = root
        .GetNamedObject(&HSTRING::from("tracks"))
        .and_then(|tracks| tracks.GetNamedArray(&HSTRING::from("items")))
        .map_err(|_| ApiError::invalid())?;
    let mut songs = Vec::new();
    for index in 0..found(&items)? {
        let item = items.GetObjectAt(index).map_err(|_| ApiError::invalid())?;
        let uri = string(&item, "uri")?;
        if !valid_track_uri(&uri) || songs.iter().any(|song: &Song| song.uri.as_ref() == uri) {
            continue;
        }
        let artist = artist_names(&item)?;
        let album = item
            .GetNamedObject(&HSTRING::from("album"))
            .map_err(|_| ApiError::invalid())?;
        let artwork = artwork(&album);
        // A song whose album has no usable address still plays, on its own.
        let album_uri = string(&album, "uri")
            .ok()
            .filter(|uri| valid_album_uri(uri))
            .map(Arc::from);
        songs.push(Song {
            uri: uri.into(),
            title: bounded(string(&item, "name")?),
            artist: bounded(artist),
            album: bounded(string(&album, "name")?),
            album_uri,
            artwork,
        });
    }
    Ok(songs)
}

/// How many rows of a search answer are read.
fn found(items: &JsonArray) -> ApiResult<u32> {
    Ok(items
        .Size()
        .map_err(|_| ApiError::invalid())?
        .min(SEARCH_RESULTS))
}

/// "Muse, Queen": the names in a song's or an album's `artists`.
fn artist_names(item: &JsonObject) -> ApiResult<String> {
    let artists = item
        .GetNamedArray(&HSTRING::from("artists"))
        .map_err(|_| ApiError::invalid())?;
    let mut names = Vec::new();
    for artist in 0..artists.Size().map_err(|_| ApiError::invalid())?.min(16) {
        names.push(string(
            &artists
                .GetObjectAt(artist)
                .map_err(|_| ApiError::invalid())?,
            "name",
        )?);
    }
    Ok(names.join(", "))
}

/// The first of an item's pictures, when it is on one of Spotify's picture hosts.
fn artwork(item: &JsonObject) -> Option<Arc<str>> {
    item.GetNamedArray(&HSTRING::from("images"))
        .ok()
        .and_then(|images| images.GetObjectAt(0).ok())
        .and_then(|image| string(&image, "url").ok())
        .filter(|url| artwork_location(url).is_some())
        .map(Arc::from)
}

/// A count Spotify gives as a number, when it is one a count can be.
fn whole_number(item: &JsonObject, name: &str) -> Option<u32> {
    item.GetNamedNumber(&HSTRING::from(name))
        .ok()
        .filter(|total| (0.0..=f64::from(u32::MAX)).contains(total))
        .map(|total| total as u32)
}

/// The year an album came out. Spotify gives the day, the month or only the year, each
/// starting with the year.
fn release_year(item: &JsonObject) -> Option<u16> {
    string(item, "release_date")
        .ok()?
        .get(..4)?
        .parse()
        .ok()
        .filter(|year| *year > 0)
}

/// The albums or artists in a search answer. Spotify leaves empty places in these lists; a
/// row without a usable address or a name, or listed twice, is left out. An artist has none
/// of what an album adds, so the same reading serves both.
fn parse_collections(bytes: &[u8], kind: CatalogKind) -> ApiResult<Vec<Collection>> {
    let (list, kind, valid): (&str, CollectionKind, fn(&str) -> bool) = match kind {
        CatalogKind::Albums => ("albums", CollectionKind::Album, valid_album_uri),
        CatalogKind::Artists => ("artists", CollectionKind::Artist, valid_artist_uri),
        CatalogKind::Songs => return Err(ApiError::invalid()),
    };
    let items = parse_json(bytes)?
        .GetNamedObject(&HSTRING::from(list))
        .and_then(|list| list.GetNamedArray(&HSTRING::from("items")))
        .map_err(|_| ApiError::invalid())?;
    let mut collections: Vec<Collection> = Vec::new();
    for index in 0..found(&items)? {
        let Ok(item) = items.GetObjectAt(index) else {
            continue;
        };
        let Some(uri) = string(&item, "uri").ok().filter(|uri| valid(uri)) else {
            continue;
        };
        let name = string(&item, "name").map(bounded).unwrap_or_default();
        if name.trim().is_empty() || collections.iter().any(|known| known.uri.as_ref() == uri) {
            continue;
        }
        collections.push(Collection {
            kind,
            uri: uri.into(),
            name,
            by: artist_names(&item).map(bounded).unwrap_or_default(),
            year: release_year(&item),
            songs: whole_number(&item, "total_tracks"),
            artwork: artwork(&item),
        });
    }
    Ok(collections)
}

/// One page of the person's library, and whether Spotify has more.
struct PlaylistPage {
    playlists: Vec<Collection>,
    more: bool,
}

fn parse_playlists(bytes: &[u8]) -> ApiResult<PlaylistPage> {
    let root = parse_json(bytes)?;
    let items = root
        .GetNamedArray(&HSTRING::from("items"))
        .map_err(|_| ApiError::invalid())?;
    let count = items.Size().map_err(|_| ApiError::invalid())?;
    let mut playlists = Vec::new();
    for index in 0..count.min(PLAYLIST_PAGE as u32) {
        // Spotify leaves an empty place for a playlist it can no longer show.
        let Ok(item) = items.GetObjectAt(index) else {
            continue;
        };
        let Some(uri) = string(&item, "uri")
            .ok()
            .filter(|uri| valid_playlist_uri(uri))
        else {
            continue;
        };
        let name = string(&item, "name").map(bounded).unwrap_or_default();
        if name.trim().is_empty() {
            continue;
        }
        let owner = item
            .GetNamedObject(&HSTRING::from("owner"))
            .ok()
            .and_then(|owner| string(&owner, "display_name").ok())
            .map(bounded)
            .unwrap_or_default();
        // Spotify moved the count from `tracks` to `items`; either may be missing.
        let songs = ["items", "tracks"].into_iter().find_map(|field| {
            whole_number(&item.GetNamedObject(&HSTRING::from(field)).ok()?, "total")
        });
        playlists.push(Collection {
            kind: CollectionKind::Playlist,
            uri: uri.into(),
            name,
            by: owner,
            year: None,
            songs,
            artwork: artwork(&item),
        });
    }
    Ok(PlaylistPage {
        playlists,
        // The address of the next page, or null on the last one.
        more: root.GetNamedString(&HSTRING::from("next")).is_ok(),
    })
}

const ARTWORK_HOSTS: [&str; 2] = [".scdn.co", ".spotifycdn.com"];

/// The host and path of a cover picture, when it is on one of Spotify's picture hosts and
/// its address holds nothing but plain characters. Song covers come from `i.scdn.co`;
/// playlists also use Spotify's hosts for mosaics and generated covers.
pub fn artwork_location(url: &str) -> Option<(&str, &str)> {
    let address = url.strip_prefix("https://")?;
    let (host, path) = address.split_at(address.find('/')?);
    let trusted = ARTWORK_HOSTS
        .iter()
        .any(|suffix| host.len() > suffix.len() && host.ends_with(suffix))
        && host.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-')
        });
    let plain = path.len() > 1
        && path.len() < 512
        && !path.contains("..")
        && path
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.'));
    (trusted && plain).then_some((host, path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::VecDeque, sync::Mutex};

    struct TestStorage(PathBuf);
    impl TestStorage {
        fn new() -> Self {
            let folder = std::env::temp_dir().join(format!(
                "core-spotify-api-{}",
                super::super::encoding::random_secret().unwrap()
            ));
            std::fs::create_dir(&folder).unwrap();
            Self(folder)
        }
        fn path(&self) -> PathBuf {
            self.0.join("spotify-token.bin")
        }
    }
    impl Drop for TestStorage {
        fn drop(&mut self) {
            let _ = token_store::remove(&self.path());
            let _ = std::fs::remove_dir(&self.0);
        }
    }

    struct RecordedRequest {
        host: String,
        method: &'static str,
        path: String,
        headers: String,
        body: Vec<u8>,
    }
    struct MockTransport {
        responses: VecDeque<http::Response>,
        requests: Arc<Mutex<Vec<RecordedRequest>>>,
    }
    impl Transport for MockTransport {
        fn send(
            &mut self,
            host: &str,
            method: &'static str,
            path: &str,
            headers: &str,
            body: &[u8],
        ) -> ApiResult<http::Response> {
            self.requests.lock().unwrap().push(RecordedRequest {
                host: host.into(),
                method,
                path: path.into(),
                headers: headers.into(),
                body: body.into(),
            });
            self.responses.pop_front().ok_or_else(ApiError::network)
        }
    }

    fn response(status: u32, body: &str, retry_after: Option<u32>) -> http::Response {
        http::Response {
            status,
            body: body.as_bytes().to_vec(),
            location: None,
            retry_after,
        }
    }

    fn devices_response() -> http::Response {
        response(
            200,
            r#"{"devices":[{"id":"pc_device","name":"This PC","type":"Computer","is_active":false,"is_restricted":false}]}"#,
            None,
        )
    }

    fn playback_response(uri: &str, playing: bool) -> http::Response {
        response(
            200,
            &format!(
                r#"{{"device":{{"id":"pc_device"}},"is_playing":{playing},"item":{{"uri":"{uri}"}}}}"#
            ),
            None,
        )
    }

    fn mock_client(
        storage: &TestStorage,
        responses: Vec<http::Response>,
    ) -> (Client, Arc<Mutex<Vec<RecordedRequest>>>) {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let mut client = Client::new(
            SpotifySettings {
                enabled: true,
                client_id: "a".repeat(32),
                volume: true,
            },
            storage.path(),
        );
        client.tokens = Some(Tokens {
            access: "initial-access".into(),
            refresh: "original-refresh".into(),
            expires: Instant::now() + Duration::from_secs(3600),
            scopes: None,
        });
        client.transport = Box::new(MockTransport {
            responses: responses.into(),
            requests: requests.clone(),
        });
        (client, requests)
    }
    #[test]
    fn catalog_responses_produce_validated_playable_rows() {
        let _runtime = crate::windows::TestRuntime::enter();
        let bytes = br#"{"tracks":{"items":[{"uri":"spotify:track:0123456789abcdefghijkl","name":"Song","artists":[{"name":"Artist"}],"album":{"name":"Album","uri":"spotify:album:abcdefghijkl0123456789","images":[{"url":"https://i.scdn.co/image/abc"}]}},{"uri":"spotify:track:abcdefghijkl0123456789","name":"Other","artists":[{"name":"Artist"}],"album":{"name":"Album","uri":"spotify:album:bad\",\"uris\":[]"}},{"uri":"spotify:track:ABCDEFGHIJKL0123456789","name":"Third","artists":[{"name":"Artist"}],"album":{"name":"Album"}}]}}"#;
        let songs = parse_songs(bytes).unwrap_or_else(|_| panic!("valid catalog response"));
        assert_eq!(songs[0].title.as_ref(), "Song");
        assert_eq!(songs[0].artist.as_ref(), "Artist");
        assert_eq!(
            songs[0].album_uri.as_deref(),
            Some("spotify:album:abcdefghijkl0123456789")
        );
        assert!(songs[0].artwork.is_some());
        // An album address that is malformed or missing leaves a song that plays on its own.
        assert_eq!(songs.len(), 3);
        assert!(songs[1].album_uri.is_none() && songs[2].album_uri.is_none());
        assert!(!valid_album_uri("spotify:track:0123456789abcdefghijkl"));
        assert!(!valid_track_uri("spotify:track:bad\"injection"));
        assert!(artwork_location("https://i.scdn.co.evil/image/abc").is_none());
        assert!(parse_songs(br#"{"tracks":{"items":[{}]}}"#).is_err());
    }
    #[test]
    fn album_and_artist_answers_become_rows_that_play_them() {
        let _runtime = crate::windows::TestRuntime::enter();
        let albums = br#"{"albums":{"items":[{"uri":"spotify:album:0123456789abcdefghijkl","name":"Absolution","artists":[{"name":"Muse"},{"name":"Guest"}],"release_date":"2003-09-15","total_tracks":14,"images":[{"url":"https://i.scdn.co/image/abc"}]},null,{"uri":"spotify:album:bad\"id","name":"Injected"},{"uri":"spotify:album:abcdefghijkl0123456789","name":"  "},{"uri":"spotify:album:0123456789abcdefghijkl","name":"Listed twice"},{"uri":"spotify:album:ABCDEFGHIJKL0123456789","name":"Bare","release_date":"1997","images":[{"url":"https://evil.example/image/abc"}]},{"uri":"spotify:artist:0123456789abcdefghijkl","name":"Not an album"}]}}"#;
        let found = parse_collections(albums, CatalogKind::Albums)
            .unwrap_or_else(|_| panic!("valid album answer"));
        let names: Vec<&str> = found.iter().map(|album| &*album.name).collect();
        // An empty place, a malformed address, a nameless album, one listed twice and
        // something that is not an album are left out.
        assert_eq!(names, ["Absolution", "Bare"]);
        assert_eq!(
            found[0],
            Collection {
                kind: CollectionKind::Album,
                uri: "spotify:album:0123456789abcdefghijkl".into(),
                name: "Absolution".into(),
                by: "Muse, Guest".into(),
                year: Some(2003),
                songs: Some(14),
                artwork: Some("https://i.scdn.co/image/abc".into()),
            }
        );
        // Whatever Spotify leaves out is simply not shown, and a picture from elsewhere
        // is never fetched.
        assert_eq!(
            (&*found[1].by, found[1].year, found[1].songs),
            ("", Some(1997), None)
        );
        assert!(found[1].artwork.is_none());

        let artists = br#"{"artists":{"items":[{"uri":"spotify:artist:0123456789abcdefghijkl","name":"Muse","images":[{"url":"https://i.scdn.co/image/def"}],"followers":{"total":1}},{"uri":"spotify:album:0123456789abcdefghijkl","name":"Not an artist"}]}}"#;
        let found = parse_collections(artists, CatalogKind::Artists)
            .unwrap_or_else(|_| panic!("valid artist answer"));
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0],
            Collection {
                kind: CollectionKind::Artist,
                uri: "spotify:artist:0123456789abcdefghijkl".into(),
                name: "Muse".into(),
                by: "".into(),
                year: None,
                songs: None,
                artwork: Some("https://i.scdn.co/image/def".into()),
            }
        );
        // An answer of another kind is an error, not an empty list.
        assert!(parse_collections(artists, CatalogKind::Albums).is_err());
        assert!(parse_collections(albums, CatalogKind::Songs).is_err());
        // Every kind Spotify plays as a whole is accepted for playing, and nothing else.
        for uri in [
            "spotify:playlist:0123456789abcdefghijkl",
            "spotify:album:0123456789abcdefghijkl",
            "spotify:artist:0123456789abcdefghijkl",
        ] {
            assert!(valid_collection_uri(uri), "{uri}");
        }
        for uri in [
            "spotify:track:0123456789abcdefghijkl",
            "spotify:show:0123456789abcdefghijkl",
            "spotify:artist:short",
            "spotify:album:0123456789abcdefghijk\"",
        ] {
            assert!(!valid_collection_uri(uri), "{uri}");
        }
    }

    #[test]
    fn albums_and_artists_are_searched_by_their_own_type_with_the_text_encoded() {
        let _runtime = crate::windows::TestRuntime::enter();
        let storage = TestStorage::new();
        let (mut client, requests) = mock_client(
            &storage,
            vec![
                response(200, r#"{"albums":{"items":[]}}"#, None),
                response(200, r#"{"artists":{"items":[]}}"#, None),
            ],
        );
        assert!(client
            .search_collections(CatalogKind::Albums, "Björk & Jóga")
            .is_ok_and(|albums| albums.is_empty()));
        assert!(client
            .search_collections(CatalogKind::Artists, "Muse")
            .is_ok_and(|artists| artists.is_empty()));
        let requests = requests.lock().unwrap();
        assert_eq!(
            requests[0].path,
            "/v1/search?type=album&limit=10&q=Bj%C3%B6rk%20%26%20J%C3%B3ga"
        );
        assert_eq!(requests[1].path, "/v1/search?type=artist&limit=10&q=Muse");
        assert!(requests.iter().all(|request| request.method == "GET"
            && request.host == "api.spotify.com"
            && request.body.is_empty()));
    }

    const PLAYLIST: &str = "spotify:playlist:0123456789abcdefghijkl";

    /// A playlist as Spotify lists it in a library, its address made of `id` 22 times.
    fn playlist_item(id: char, name: &str) -> String {
        format!(
            r#"{{"uri":"spotify:playlist:{}","name":"{name}","owner":{{"display_name":"Robert"}},"items":{{"total":42}},"images":[{{"url":"https://mosaic.scdn.co/640/abc123"}}]}}"#,
            id.to_string().repeat(22)
        )
    }

    /// What Spotify reports while it plays from `context`.
    fn context_response(context: &str, playing: bool) -> http::Response {
        response(
            200,
            &format!(
                r#"{{"device":{{"id":"pc_device"}},"is_playing":{playing},"item":{{"uri":"spotify:track:0123456789abcdefghijkl"}},"context":{{"uri":"{context}"}}}}"#
            ),
            None,
        )
    }

    #[test]
    fn the_library_is_read_page_by_page_and_only_usable_playlists_are_listed() {
        let _runtime = crate::windows::TestRuntime::enter();
        let storage = TestStorage::new();
        let first = format!(
            r#"{{"items":[{},null,{{"uri":"spotify:playlist:bad\"id","name":"Injected"}},{{"uri":"spotify:playlist:{}","name":"  "}},{{"uri":"spotify:playlist:{}","name":"Old shape","owner":{{"display_name":null}},"tracks":{{"total":7}},"images":null}}],"next":"https://api.spotify.com/v1/me/playlists?offset=50&limit=50"}}"#,
            playlist_item('a', "Late Night Chill"),
            "b".repeat(22),
            "c".repeat(22),
        );
        let second = format!(
            r#"{{"items":[{},{}],"next":null}}"#,
            playlist_item('d', "Gym"),
            playlist_item('a', "Late Night Chill"),
        );
        let (mut client, requests) = mock_client(
            &storage,
            vec![response(200, &first, None), response(200, &second, None)],
        );
        let playlists = client.playlists(&|| false).unwrap();
        // A place Spotify left empty, a malformed address, a nameless playlist and one listed
        // twice are left out.
        let names: Vec<&str> = playlists.iter().map(|playlist| &*playlist.name).collect();
        assert_eq!(names, ["Late Night Chill", "Old shape", "Gym"]);
        assert_eq!(
            &*playlists[0].uri,
            format!("spotify:playlist:{}", "a".repeat(22))
        );
        assert_eq!(
            (&*playlists[0].by, playlists[0].songs),
            ("Robert", Some(42))
        );
        assert!(playlists
            .iter()
            .all(|playlist| playlist.kind == CollectionKind::Playlist && playlist.year.is_none()));
        assert_eq!(
            playlists[0].artwork.as_deref(),
            Some("https://mosaic.scdn.co/640/abc123")
        );
        // The count's older place is still read, and a missing owner or picture is no error.
        assert_eq!((&*playlists[1].by, playlists[1].songs), ("", Some(7)));
        assert!(playlists[1].artwork.is_none());
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].path, "/v1/me/playlists?limit=50&offset=0");
        assert_eq!(requests[1].path, "/v1/me/playlists?limit=50&offset=50");
        assert!(requests.iter().all(|request| request.method == "GET"
            && request.host == "api.spotify.com"
            && request.headers.contains("Bearer initial-access")));
        // A response that is not a library is an error, not an empty library.
        assert!(parse_playlists(br#"{"tracks":{}}"#).is_err());
    }

    #[test]
    fn a_connection_made_before_playlists_is_asked_to_connect_again_without_asking_spotify() {
        let _runtime = crate::windows::TestRuntime::enter();
        let storage = TestStorage::new();
        let empty = r#"{"items":[],"next":null}"#;
        let (mut client, requests) = mock_client(
            &storage,
            vec![response(200, empty, None), response(200, empty, None)],
        );
        let allow = |client: &mut Client, scopes: Option<&str>| {
            client.tokens.as_mut().unwrap().scopes = scopes.map(str::to_owned);
        };
        allow(
            &mut client,
            Some("user-modify-playback-state user-read-playback-state"),
        );
        let refused = client.playlists(&|| false).unwrap_err();
        assert_eq!(refused.status, SongStatus::PermissionRequired);
        assert!(requests.lock().unwrap().is_empty());
        // With the permission, and for a token that does not say, Spotify is asked.
        allow(
            &mut client,
            Some("playlist-read-collaborative playlist-read-private user-read-playback-state"),
        );
        assert!(client.playlists(&|| false).unwrap().is_empty());
        allow(&mut client, None);
        assert!(client.playlists(&|| false).unwrap().is_empty());
        assert_eq!(requests.lock().unwrap().len(), 2);
    }

    #[test]
    fn what_a_token_may_do_is_kept_through_a_refresh_that_does_not_repeat_it() {
        let _runtime = crate::windows::TestRuntime::enter();
        let storage = TestStorage::new();
        let listed = r#"{"access_token":"first-access","refresh_token":"first-refresh","token_type":"Bearer","expires_in":3600,"scope":"playlist-read-private user-read-playback-state"}"#;
        let silent = r#"{"access_token":"second-access","token_type":"Bearer","expires_in":3600}"#;
        let (mut client, _requests) = mock_client(
            &storage,
            vec![response(200, listed, None), response(200, silent, None)],
        );
        for refresh in ["original-refresh", "first-refresh"] {
            client
                .receive_tokens("grant_type=refresh_token", Some(refresh.into()))
                .unwrap();
            assert!(client.granted("playlist-read-private"), "{refresh}");
            assert!(!client.granted("playlist-modify-public"), "{refresh}");
        }
    }

    #[test]
    fn enter_plays_the_chosen_playlist_and_confirms_it_by_what_spotify_plays_from() {
        let _runtime = crate::windows::TestRuntime::enter();
        let storage = TestStorage::new();
        let (mut client, requests) = mock_client(
            &storage,
            vec![
                devices_response(),
                response(204, "", None),
                context_response(PLAYLIST, true),
            ],
        );
        assert_eq!(
            client
                .play_collection(PLAYLIST, PlayMode::AsItIs, &|| false)
                .unwrap()
                .name
                .as_ref(),
            "This PC"
        );
        // Anything that is not a playlist's address is refused before a request is made.
        for invalid in [
            "spotify:track:0123456789abcdefghijkl",
            "spotify:playlist:short",
            "spotify:playlist:0123456789abcdefghij\"}",
        ] {
            assert!(
                client
                    .play_collection(invalid, PlayMode::Shuffled, &|| false)
                    .is_err(),
                "{invalid}"
            );
        }
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].path, "/v1/me/player/devices");
        assert_eq!(requests[1].method, "PUT");
        assert_eq!(requests[1].path, "/v1/me/player/play?device_id=pc_device");
        assert_eq!(
            requests[1].body,
            br#"{"context_uri":"spotify:playlist:0123456789abcdefghijkl"}"#
        );
        assert_eq!(requests[2].path, "/v1/me/player");
    }

    /// What Spotify reports while it plays the playlist with these settings.
    fn settings_response(shuffle: bool, repeat: &str) -> http::Response {
        response(
            200,
            &format!(
                r#"{{"device":{{"id":"pc_device"}},"is_playing":true,"shuffle_state":{shuffle},"repeat_state":"{repeat}","item":{{"uri":"spotify:track:0123456789abcdefghijkl"}},"context":{{"uri":"{PLAYLIST}"}}}}"#
            ),
            None,
        )
    }

    /// Starts the playlist without the real pauses between confirmation readings.
    fn start_playlist(
        client: &mut Client,
        mode: PlayMode,
    ) -> ApiResult<super::super::playback::PlaybackTarget> {
        super::super::playback::play_collection(client, PLAYLIST, mode, "This PC", |_| {}, &|| {
            false
        })
    }

    fn sent(requests: &Mutex<Vec<RecordedRequest>>) -> Vec<(&'static str, String)> {
        requests
            .lock()
            .unwrap()
            .iter()
            .map(|request| (request.method, request.path.clone()))
            .collect()
    }

    const SHUFFLE_REQUEST: &str = "/v1/me/player/shuffle?state=true&device_id=pc_device";
    const REPEAT_REQUEST: &str = "/v1/me/player/repeat?state=context&device_id=pc_device";
    const PLAY_REQUEST: &str = "/v1/me/player/play?device_id=pc_device";

    #[test]
    fn shuffle_is_asked_for_again_once_the_playlist_plays_until_spotify_reports_it() {
        let _runtime = crate::windows::TestRuntime::enter();
        let storage = TestStorage::new();
        let (mut client, requests) = mock_client(
            &storage,
            vec![
                devices_response(),
                // An idle device refuses shuffle before anything plays; that is not an error.
                response(404, "", None),
                response(204, "", None),
                // Spotify accepts the setting twice before its app takes it.
                settings_response(false, "off"),
                response(204, "", None),
                settings_response(false, "off"),
                response(204, "", None),
                settings_response(true, "off"),
            ],
        );
        assert!(start_playlist(&mut client, PlayMode::Shuffled).is_ok());
        let put = |path: &str| ("PUT", path.to_owned());
        let read = ("GET", "/v1/me/player".to_owned());
        assert_eq!(
            sent(&requests),
            [
                ("GET", "/v1/me/player/devices".to_owned()),
                put(SHUFFLE_REQUEST),
                put(PLAY_REQUEST),
                read.clone(),
                put(SHUFFLE_REQUEST),
                read.clone(),
                put(SHUFFLE_REQUEST),
                read,
            ]
        );
        // The setting carries nothing but its address.
        assert!(requests.lock().unwrap()[4].body.is_empty());
    }

    #[test]
    fn a_playlist_that_starts_shuffled_is_asked_nothing_more() {
        let _runtime = crate::windows::TestRuntime::enter();
        let storage = TestStorage::new();
        let (mut client, requests) = mock_client(
            &storage,
            vec![
                devices_response(),
                response(204, "", None),
                response(204, "", None),
                settings_response(true, "off"),
            ],
        );
        assert!(start_playlist(&mut client, PlayMode::Shuffled).is_ok());
        assert_eq!(sent(&requests).len(), 4);
    }

    #[test]
    fn repeat_is_set_once_the_playlist_plays_and_a_refusal_is_reported() {
        let _runtime = crate::windows::TestRuntime::enter();
        let storage = TestStorage::new();
        let (mut client, requests) = mock_client(
            &storage,
            vec![
                devices_response(),
                response(204, "", None),
                settings_response(false, "off"),
                response(204, "", None),
                // Repeating one song is not what was asked for.
                settings_response(false, "track"),
                response(204, "", None),
                settings_response(false, "context"),
                // A second try, which Spotify lets start but not repeat.
                devices_response(),
                response(204, "", None),
                settings_response(false, "off"),
                response(403, "", None),
            ],
        );
        assert!(start_playlist(&mut client, PlayMode::Looped).is_ok());
        let first: Vec<(&str, String)> = sent(&requests);
        assert_eq!(first.len(), 7);
        // Nothing is asked before the playlist starts: repeat does not choose the first song.
        assert_eq!(first[1], ("PUT", PLAY_REQUEST.to_owned()));
        assert_eq!(first[3], ("PUT", REPEAT_REQUEST.to_owned()));
        assert_eq!(first[5], ("PUT", REPEAT_REQUEST.to_owned()));

        let refused = start_playlist(&mut client, PlayMode::Looped).unwrap_err();
        assert_eq!(refused.status, SongStatus::AccessDenied);
        assert!(refused.message.contains("shuffle or repeat"));
        // Nothing was read or sent again after the refusal.
        assert_eq!(sent(&requests).len(), 11);
    }

    #[test]
    fn a_setting_spotify_accepts_but_never_takes_is_not_reported_as_done() {
        let _runtime = crate::windows::TestRuntime::enter();
        let storage = TestStorage::new();
        let mut responses = vec![devices_response(), response(204, "", None)];
        for _ in 0..4 {
            responses.push(settings_response(false, "off"));
            responses.push(response(204, "", None));
        }
        responses.push(settings_response(false, "off"));
        let (mut client, requests) = mock_client(&storage, responses);

        let failed = start_playlist(&mut client, PlayMode::Looped).unwrap_err();

        assert!(failed.message.contains("plays, but Spotify did not turn"));
        let sent = sent(&requests);
        // Asked after every reading but the last, which nothing could follow.
        assert_eq!(
            sent.iter()
                .filter(|(_, path)| path == REPEAT_REQUEST)
                .count(),
            4
        );
        // The playlist itself was started once and never restarted.
        assert_eq!(
            sent.iter().filter(|(_, path)| path == PLAY_REQUEST).count(),
            1
        );
        assert_eq!(sent.len(), 11);
    }

    #[test]
    fn something_else_still_playing_is_not_reported_as_the_chosen_playlist() {
        let _runtime = crate::windows::TestRuntime::enter();
        let storage = TestStorage::new();
        let mut responses = vec![devices_response(), response(204, "", None)];
        responses
            .extend((0..5).map(|_| context_response("spotify:album:abcdefghijkl0123456789", true)));
        let (mut client, requests) = mock_client(&storage, responses);

        let result = super::super::playback::play_collection(
            &mut client,
            PLAYLIST,
            PlayMode::AsItIs,
            "This PC",
            |_| {},
            &|| false,
        );

        assert!(result
            .unwrap_err()
            .message
            .contains("did not start the playlist"));
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 7);
        // What plays instead is never restarted on the person's behalf.
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.method == "PUT")
                .count(),
            1
        );
    }

    #[test]
    fn cover_pictures_are_fetched_only_from_spotifys_picture_hosts() {
        for (url, location) in [
            (
                "https://i.scdn.co/image/ab67616d0000b273abc",
                ("i.scdn.co", "/image/ab67616d0000b273abc"),
            ),
            (
                "https://mosaic.scdn.co/640/ab67616d00001e02abc",
                ("mosaic.scdn.co", "/640/ab67616d00001e02abc"),
            ),
            (
                "https://image-cdn-ak.spotifycdn.com/image/ab67706c0000da84abc",
                ("image-cdn-ak.spotifycdn.com", "/image/ab67706c0000da84abc"),
            ),
            (
                "https://pickasso.spotifycdn.com/image/ab67c0de0000deef/dt/v1/img/daily/1/abc/en",
                (
                    "pickasso.spotifycdn.com",
                    "/image/ab67c0de0000deef/dt/v1/img/daily/1/abc/en",
                ),
            ),
        ] {
            assert_eq!(artwork_location(url), Some(location), "{url}");
        }
        for url in [
            "http://i.scdn.co/image/abc",
            "https://i.scdn.co.evil/image/abc",
            "https://evil.example/i.scdn.co/image/abc",
            "https://scdn.co/image/abc",
            "https://.scdn.co/image/abc",
            "https://i.scdn.co@evil.example/image/abc",
            "https://i.scdn.co:8443/image/abc",
            "https://I.SCDN.CO/image/abc",
            "https://i.scdn.co",
            "https://i.scdn.co/",
            "https://i.scdn.co/image/../secret",
            "https://i.scdn.co/image/abc?size=1",
            "https://i.scdn.co/image/a b",
        ] {
            assert!(artwork_location(url).is_none(), "{url}");
        }
        let long = format!("https://i.scdn.co/{}", "a".repeat(600));
        assert!(artwork_location(&long).is_none());
    }

    #[test]
    fn api_failures_explain_auth_permissions_devices_and_backoff() {
        assert_eq!(
            response_error(401).unwrap().status,
            SongStatus::ConnectRequired
        );
        assert_eq!(
            response_error(403).unwrap().status,
            SongStatus::AccessDenied
        );
        assert_eq!(response_error(429).unwrap().status, SongStatus::RateLimited);
        assert!(response_error(404)
            .unwrap()
            .message
            .contains("Open Spotify"));
        assert!(response_error(204).is_none());
    }

    #[test]
    fn catalog_search_encodes_unicode_and_authenticates_only_to_spotify() {
        let _runtime = crate::windows::TestRuntime::enter();
        let storage = TestStorage::new();
        let (mut client, requests) = mock_client(
            &storage,
            vec![response(200, r#"{"tracks":{"items":[]}}"#, None)],
        );
        assert!(client.search("Björk & Jóga").is_ok());
        let requests = requests.lock().unwrap();
        assert_eq!(requests[0].host, "api.spotify.com");
        assert_eq!(requests[0].method, "GET");
        assert_eq!(
            requests[0].path,
            "/v1/search?type=track&limit=10&q=Bj%C3%B6rk%20%26%20J%C3%B3ga"
        );
        assert!(requests[0]
            .headers
            .contains("Authorization: Bearer initial-access\r\n"));
        assert!(requests[0].body.is_empty());
    }

    #[test]
    fn enter_plays_exactly_the_selected_track_and_rejects_invalid_uris_before_sending() {
        let _runtime = crate::windows::TestRuntime::enter();
        let storage = TestStorage::new();
        let uri = "spotify:track:0123456789abcdefghijkl";
        let album = "spotify:album:abcdefghijkl0123456789";
        let started = || {
            vec![
                devices_response(),
                response(204, "", None),
                playback_response(uri, true),
            ]
        };
        let mut responses = started();
        responses.extend(started());
        responses.extend(started());
        let (mut client, requests) = mock_client(&storage, responses);
        assert_eq!(
            client
                .play(uri, Some(album), &|| false)
                .unwrap()
                .name
                .as_ref(),
            "This PC"
        );
        assert!(client
            .play("spotify:track:invalid", Some(album), &|| false)
            .is_err());
        // Without an album, or with an address that is not one, the song is asked for on its
        // own; a malformed address is never written into the request.
        assert!(client.play(uri, None, &|| false).is_ok());
        assert!(client
            .play(uri, Some("spotify:album:bad\",\"uris\":[]"), &|| false)
            .is_ok());
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 9);
        assert_eq!(requests[0].method, "GET");
        assert_eq!(requests[0].path, "/v1/me/player/devices");
        assert_eq!(requests[1].method, "PUT");
        assert_eq!(requests[1].path, "/v1/me/player/play?device_id=pc_device");
        // Inside its album, at the song, from its start: the form the desktop app plays.
        assert_eq!(
            requests[1].body,
            br#"{"context_uri":"spotify:album:abcdefghijkl0123456789","offset":{"uri":"spotify:track:0123456789abcdefghijkl"},"position_ms":0}"#
        );
        assert_eq!(requests[2].method, "GET");
        assert_eq!(requests[2].path, "/v1/me/player");
        for alone in [&requests[4], &requests[7]] {
            assert_eq!(
                alone.body,
                br#"{"uris":["spotify:track:0123456789abcdefghijkl"],"position_ms":0}"#
            );
        }
    }

    #[test]
    fn expired_access_is_refreshed_once_and_rotated_credentials_are_saved() {
        let _runtime = crate::windows::TestRuntime::enter();
        let storage = TestStorage::new();
        let tokens = r#"{"access_token":"replacement-access","refresh_token":"replacement-refresh","token_type":"Bearer","expires_in":3600}"#;
        let (mut client, requests) = mock_client(
            &storage,
            vec![
                response(401, "", None),
                response(200, tokens, None),
                devices_response(),
                response(204, "", None),
                playback_response("spotify:track:0123456789abcdefghijkl", true),
            ],
        );
        assert!(client
            .play("spotify:track:0123456789abcdefghijkl", None, &|| false)
            .is_ok());
        assert_eq!(
            token_store::load(&storage.path(), &client.settings.client_id)
                .unwrap()
                .as_deref(),
            Some("replacement-refresh")
        );
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 5);
        assert_eq!(requests[1].host, "accounts.spotify.com");
        assert_eq!(requests[1].method, "POST");
        assert!(std::str::from_utf8(&requests[1].body)
            .unwrap()
            .contains("refresh_token=original-refresh"));
        assert!(requests[2].headers.contains("Bearer replacement-access"));
    }

    #[test]
    fn choosing_the_web_player_in_spotify_keeps_playback_off_the_inactive_desktop() {
        let _runtime = crate::windows::TestRuntime::enter();
        let storage = TestStorage::new();
        let devices = r#"{"devices":[{"id":"pc_device","name":"This PC","type":"Computer","is_active":false,"is_restricted":false},{"id":"web_device","name":"Web Player (Chrome)","type":"Computer","is_active":true,"is_restricted":false}]}"#;
        let playback = r#"{"device":{"id":"web_device"},"is_playing":true,"item":{"uri":"spotify:track:0123456789abcdefghijkl"}}"#;
        let (mut client, requests) = mock_client(
            &storage,
            vec![
                response(200, devices, None),
                response(204, "", None),
                response(200, playback, None),
            ],
        );

        let result = super::super::playback::play_song(
            &mut client,
            "spotify:track:0123456789abcdefghijkl",
            None,
            "This PC",
            |_| {},
            &|| false,
        );

        assert_eq!(result.unwrap().name.as_ref(), "Web Player (Chrome)");
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[1].method, "PUT");
        assert_eq!(requests[1].path, "/v1/me/player/play?device_id=web_device");
        assert_eq!(
            requests[1].body,
            br#"{"uris":["spotify:track:0123456789abcdefghijkl"],"position_ms":0}"#
        );
    }

    #[test]
    fn a_successful_http_response_without_playback_is_reported_as_a_failure() {
        let _runtime = crate::windows::TestRuntime::enter();
        let storage = TestStorage::new();
        let mut responses = vec![devices_response(), response(204, "", None)];
        responses.extend((0..5).map(|_| response(204, "", None)));
        let (mut client, requests) = mock_client(&storage, responses);

        let result = super::super::playback::play_song(
            &mut client,
            "spotify:track:0123456789abcdefghijkl",
            None,
            "This PC",
            |_| {},
            &|| false,
        );

        assert!(result.unwrap_err().message.contains("did not start"));
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 7);
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.method == "PUT")
                .count(),
            1
        );
    }

    #[test]
    fn a_selected_track_that_remains_paused_is_resumed_once_on_the_same_device() {
        let _runtime = crate::windows::TestRuntime::enter();
        let storage = TestStorage::new();
        let uri = "spotify:track:0123456789abcdefghijkl";
        let (mut client, requests) = mock_client(
            &storage,
            vec![
                devices_response(),
                response(204, "", None),
                playback_response(uri, false),
                playback_response(uri, false),
                response(204, "", None),
                playback_response(uri, true),
            ],
        );

        let result = super::super::playback::play_song(
            &mut client,
            uri,
            Some("spotify:album:abcdefghijkl0123456789"),
            "This PC",
            |_| {},
            &|| false,
        );

        assert!(result.is_ok());
        let requests = requests.lock().unwrap();
        let commands: Vec<_> = requests
            .iter()
            .filter(|request| request.method == "PUT")
            .collect();
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[0].path, commands[1].path);
        // The retry asks for the same song inside the same album again.
        assert_eq!(commands[1].body, commands[0].body);
        assert!(commands[0].body.starts_with(br#"{"context_uri":"#));
    }

    #[test]
    fn the_sliders_volume_is_read_from_and_set_on_the_active_device() {
        use super::super::volume::{read_volume, set_volume};
        let _runtime = crate::windows::TestRuntime::enter();
        let storage = TestStorage::new();
        let (mut client, requests) = mock_client(
            &storage,
            vec![
                response(
                    200,
                    r#"{"device":{"id":"pc_device","volume_percent":64,"supports_volume":true},"is_playing":true}"#,
                    None,
                ),
                response(204, "", None),
            ],
        );
        assert_eq!(read_volume(&mut client).unwrap(), 64);
        // Levels above full, as a careless caller might send, are full.
        assert!(set_volume(&mut client, 130, &|| false).is_ok());
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].method, "GET");
        assert_eq!(requests[0].path, "/v1/me/player");
        assert_eq!(requests[1].host, "api.spotify.com");
        assert_eq!(requests[1].method, "PUT");
        assert_eq!(requests[1].path, "/v1/me/player/volume?volume_percent=100");
        assert!(requests[1].body.is_empty());
        assert!(requests[1]
            .headers
            .contains("Authorization: Bearer initial-access\r\n"));
    }

    #[test]
    fn a_missing_or_fixed_volume_is_explained_instead_of_shown_as_zero() {
        use super::super::volume::{read_volume, set_volume};
        let _runtime = crate::windows::TestRuntime::enter();
        let storage = TestStorage::new();
        let (mut client, requests) = mock_client(
            &storage,
            vec![
                // Nothing is playing anywhere.
                response(204, "", None),
                response(
                    200,
                    r#"{"device":{"id":"tv","volume_percent":100,"supports_volume":false}}"#,
                    None,
                ),
                response(
                    200,
                    r#"{"device":{"id":"cast","volume_percent":null}}"#,
                    None,
                ),
                response(200, r#"{"is_playing":true}"#, None),
                // A free account may read the volume but not change it.
                response(403, "", None),
            ],
        );
        assert!(read_volume(&mut client)
            .unwrap_err()
            .message
            .contains("No Spotify device is active"));
        for _ in 0..2 {
            assert!(read_volume(&mut client)
                .unwrap_err()
                .message
                .contains("does not let apps change its volume"));
        }
        assert!(read_volume(&mut client)
            .unwrap_err()
            .message
            .contains("invalid response"));
        let refused = set_volume(&mut client, 40, &|| false).unwrap_err();
        assert_eq!(refused.status, SongStatus::AccessDenied);
        // A cancelled change sends nothing.
        assert!(set_volume(&mut client, 40, &|| true).is_err());
        assert_eq!(requests.lock().unwrap().len(), 5);
    }

    #[test]
    fn an_old_track_is_never_resumed_or_reported_as_the_selected_song() {
        let _runtime = crate::windows::TestRuntime::enter();
        let storage = TestStorage::new();
        let mut responses = vec![devices_response(), response(204, "", None)];
        responses.extend(
            (0..5).map(|_| playback_response("spotify:track:abcdefghijkl0123456789", false)),
        );
        let (mut client, requests) = mock_client(&storage, responses);

        assert!(super::super::playback::play_song(
            &mut client,
            "spotify:track:0123456789abcdefghijkl",
            None,
            "This PC",
            |_| {},
            &|| false
        )
        .is_err());

        assert_eq!(
            requests
                .lock()
                .unwrap()
                .iter()
                .filter(|request| request.method == "PUT")
                .count(),
            1
        );
    }

    #[test]
    fn cancellation_after_device_lookup_never_sends_a_play_command() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let _runtime = crate::windows::TestRuntime::enter();
        let storage = TestStorage::new();
        let (mut client, requests) = mock_client(&storage, vec![devices_response()]);
        let checks = AtomicUsize::new(0);
        let cancelled = || checks.fetch_add(1, Ordering::SeqCst) != 0;

        assert!(super::super::playback::play_song(
            &mut client,
            "spotify:track:0123456789abcdefghijkl",
            None,
            "This PC",
            |_| {},
            &cancelled
        )
        .is_err());

        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "GET");
    }

    #[test]
    fn missing_device_permission_requests_reconnection_without_playing_anything() {
        let storage = TestStorage::new();
        let (mut client, requests) = mock_client(&storage, vec![response(403, "", None)]);

        let error = client
            .play("spotify:track:0123456789abcdefghijkl", None, &|| false)
            .unwrap_err();

        assert_eq!(error.status, SongStatus::ConnectRequired);
        assert!(error.message.contains("playback-state access"));
        assert_eq!(requests.lock().unwrap().len(), 1);
    }

    #[test]
    fn cancellation_during_token_refresh_prevents_retrying_the_play_command() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let _runtime = crate::windows::TestRuntime::enter();
        let storage = TestStorage::new();
        let tokens = r#"{"access_token":"replacement-access","refresh_token":"replacement-refresh","token_type":"Bearer","expires_in":3600}"#;
        let (mut client, requests) = mock_client(
            &storage,
            vec![
                devices_response(),
                response(401, "", None),
                response(200, tokens, None),
            ],
        );
        let checks = AtomicUsize::new(0);
        let cancelled = || checks.fetch_add(1, Ordering::SeqCst) >= 3;

        let error = super::super::playback::play_song(
            &mut client,
            "spotify:track:0123456789abcdefghijkl",
            None,
            "This PC",
            |_| {},
            &cancelled,
        )
        .unwrap_err();

        assert!(error.message.contains("cancelled"));
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.method == "PUT")
                .count(),
            1
        );
        assert_eq!(requests[2].host, "accounts.spotify.com");
    }

    #[test]
    fn restricted_devices_and_missing_or_invalid_ids_are_never_targeted() {
        let _runtime = crate::windows::TestRuntime::enter();
        let storage = TestStorage::new();
        let devices = r#"{"devices":[{"id":null,"is_restricted":false},{"id":"restricted_pc","is_restricted":true},{"id":"bad&id=other","is_restricted":false}]}"#;
        let (mut client, requests) = mock_client(&storage, vec![response(200, devices, None)]);

        assert!(client
            .play("spotify:track:0123456789abcdefghijkl", None, &|| false)
            .is_err());

        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "GET");
    }

    #[test]
    fn the_final_confirmation_read_never_sends_a_retry_it_cannot_observe() {
        let _runtime = crate::windows::TestRuntime::enter();
        let storage = TestStorage::new();
        let uri = "spotify:track:0123456789abcdefghijkl";
        let mut responses = vec![devices_response(), response(204, "", None)];
        responses.extend((0..4).map(|_| response(204, "", None)));
        responses.push(playback_response(uri, false));
        let (mut client, requests) = mock_client(&storage, responses);

        assert!(super::super::playback::play_song(
            &mut client,
            uri,
            None,
            "This PC",
            |_| {},
            &|| false
        )
        .is_err());

        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 7);
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.method == "PUT")
                .count(),
            1
        );
    }

    #[test]
    fn throttled_requests_wait_for_spotify_and_failed_tokens_never_become_credentials() {
        let _runtime = crate::windows::TestRuntime::enter();
        let storage = TestStorage::new();
        let (mut client, requests) = mock_client(&storage, vec![response(429, "", Some(3600))]);
        assert_eq!(
            client
                .play("spotify:track:0123456789abcdefghijkl", None, &|| false)
                .unwrap_err()
                .status,
            SongStatus::RateLimited
        );
        assert_eq!(
            client
                .play("spotify:track:0123456789abcdefghijkl", None, &|| false)
                .unwrap_err()
                .status,
            SongStatus::RateLimited
        );
        assert_eq!(requests.lock().unwrap().len(), 1);
        let (mut client, _) = mock_client(
            &storage,
            vec![response(
                200,
                r#"{"access_token":"bad\r\nheader","refresh_token":"refresh","token_type":"Bearer","expires_in":3600}"#,
                None,
            )],
        );
        let authorization = Authorization {
            code: "fake-code".into(),
            verifier: "fake-verifier".into(),
        };
        assert!(client.exchange_code(authorization).is_err());
        assert!(!storage.path().exists());
    }
}
