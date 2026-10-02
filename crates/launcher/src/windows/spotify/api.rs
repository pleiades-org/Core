//! The two Spotify operations Core needs: catalog search and explicit track playback.
use super::{
    authorization::{Authorization, REDIRECT_URI},
    encoding::{url_encode, valid_token},
    token_store,
};
use crate::windows::{http, settings::SpotifySettings};
use core_engine::search::{Song, SongStatus};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use windows::{core::HSTRING, Data::Json::JsonObject};

const API_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_RESPONSE_BYTES: usize = 512 * 1024;
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
        let path = format!("/v1/search?type=track&limit=10&q={}", url_encode(query));
        let response = self.authenticated("GET", &path, &[])?;
        parse_songs(&response.body).map_err(|_| ApiError::invalid())
    }

    pub fn play(
        &mut self,
        uri: &str,
        cancelled: &impl Fn() -> bool,
    ) -> ApiResult<super::playback::PlaybackTarget> {
        super::playback::play_song(
            self,
            uri,
            &std::env::var("COMPUTERNAME").unwrap_or_default(),
            super::playback::wait,
            cancelled,
        )
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
        self.tokens = Some(Tokens {
            access,
            refresh,
            expires: Instant::now() + Duration::from_secs_f64(expires),
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
    uri.strip_prefix("spotify:track:")
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
    for index in 0..items.Size().map_err(|_| ApiError::invalid())?.min(10) {
        let item = items.GetObjectAt(index).map_err(|_| ApiError::invalid())?;
        let uri = string(&item, "uri")?;
        if !valid_track_uri(&uri) || songs.iter().any(|song: &Song| song.uri.as_ref() == uri) {
            continue;
        }
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
        let album = item
            .GetNamedObject(&HSTRING::from("album"))
            .map_err(|_| ApiError::invalid())?;
        let artwork = album
            .GetNamedArray(&HSTRING::from("images"))
            .ok()
            .and_then(|images| images.GetObjectAt(0).ok())
            .and_then(|image| string(&image, "url").ok())
            .filter(|url| artwork_path(url).is_some())
            .map(Arc::from);
        songs.push(Song {
            uri: uri.into(),
            title: bounded(string(&item, "name")?),
            artist: bounded(names.join(", ")),
            album: bounded(string(&album, "name")?),
            artwork,
        });
    }
    Ok(songs)
}

pub fn artwork_path(url: &str) -> Option<&str> {
    let path = url.strip_prefix("https://i.scdn.co")?;
    (path.starts_with("/image/")
        && path.len() < 256
        && path
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_')))
    .then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::VecDeque, sync::Mutex};

    struct Apartment;
    impl Apartment {
        fn new() -> Self {
            unsafe {
                windows::Win32::System::WinRT::RoInitialize(
                    windows::Win32::System::WinRT::RO_INIT_MULTITHREADED,
                )
            }
            .unwrap();
            Self
        }
    }
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe {
                windows::Win32::System::WinRT::RoUninitialize();
            }
        }
    }

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
            },
            storage.path(),
        );
        client.tokens = Some(Tokens {
            access: "initial-access".into(),
            refresh: "original-refresh".into(),
            expires: Instant::now() + Duration::from_secs(3600),
        });
        client.transport = Box::new(MockTransport {
            responses: responses.into(),
            requests: requests.clone(),
        });
        (client, requests)
    }
    #[test]
    fn catalog_responses_produce_validated_playable_rows() {
        let _apartment = Apartment::new();
        let bytes = br#"{"tracks":{"items":[{"uri":"spotify:track:0123456789abcdefghijkl","name":"Song","artists":[{"name":"Artist"}],"album":{"name":"Album","images":[{"url":"https://i.scdn.co/image/abc"}]}}]}}"#;
        let songs = parse_songs(bytes).unwrap_or_else(|_| panic!("valid catalog response"));
        assert_eq!(songs[0].title.as_ref(), "Song");
        assert_eq!(songs[0].artist.as_ref(), "Artist");
        assert!(songs[0].artwork.is_some());
        assert!(!valid_track_uri("spotify:track:bad\"injection"));
        assert!(artwork_path("https://i.scdn.co.evil/image/abc").is_none());
        assert!(parse_songs(br#"{"tracks":{"items":[{}]}}"#).is_err());
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
        let _apartment = Apartment::new();
        let storage = TestStorage::new();
        let (mut client, requests) = mock_client(
            &storage,
            vec![response(200, r#"{"tracks":{"items":[]}}"#, None)],
        );
        assert!(client.search("Björk & Jóga").is_ok());
        let requests = requests.lock().unwrap();
        assert_eq!(requests[0].host, "api.spotify.com");
        assert_eq!(requests[0].method, "GET");
        assert!(requests[0].path.ends_with("q=Bj%C3%B6rk%20%26%20J%C3%B3ga"));
        assert!(requests[0]
            .headers
            .contains("Authorization: Bearer initial-access\r\n"));
        assert!(requests[0].body.is_empty());
    }

    #[test]
    fn enter_plays_exactly_the_selected_track_and_rejects_invalid_uris_before_sending() {
        let _apartment = Apartment::new();
        let storage = TestStorage::new();
        let uri = "spotify:track:0123456789abcdefghijkl";
        let (mut client, requests) = mock_client(
            &storage,
            vec![
                devices_response(),
                response(204, "", None),
                playback_response(uri, true),
            ],
        );
        assert_eq!(
            client.play(uri, &|| false).unwrap().name.as_ref(),
            "This PC"
        );
        assert!(client.play("spotify:track:invalid", &|| false).is_err());
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].method, "GET");
        assert_eq!(requests[0].path, "/v1/me/player/devices");
        assert_eq!(requests[1].method, "PUT");
        assert_eq!(requests[1].path, "/v1/me/player/play?device_id=pc_device");
        assert_eq!(
            requests[1].body,
            br#"{"uris":["spotify:track:0123456789abcdefghijkl"],"position_ms":0}"#
        );
        assert_eq!(requests[2].method, "GET");
        assert_eq!(requests[2].path, "/v1/me/player");
    }

    #[test]
    fn expired_access_is_refreshed_once_and_rotated_credentials_are_saved() {
        let _apartment = Apartment::new();
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
            .play("spotify:track:0123456789abcdefghijkl", &|| false)
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
        let _apartment = Apartment::new();
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
        let _apartment = Apartment::new();
        let storage = TestStorage::new();
        let mut responses = vec![devices_response(), response(204, "", None)];
        responses.extend((0..5).map(|_| response(204, "", None)));
        let (mut client, requests) = mock_client(&storage, responses);

        let result = super::super::playback::play_song(
            &mut client,
            "spotify:track:0123456789abcdefghijkl",
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
        let _apartment = Apartment::new();
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

        let result =
            super::super::playback::play_song(&mut client, uri, "This PC", |_| {}, &|| false);

        assert!(result.is_ok());
        let requests = requests.lock().unwrap();
        let commands: Vec<_> = requests
            .iter()
            .filter(|request| request.method == "PUT")
            .collect();
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[0].path, commands[1].path);
        assert_eq!(commands[1].body, commands[0].body);
    }

    #[test]
    fn an_old_track_is_never_resumed_or_reported_as_the_selected_song() {
        let _apartment = Apartment::new();
        let storage = TestStorage::new();
        let mut responses = vec![devices_response(), response(204, "", None)];
        responses.extend(
            (0..5).map(|_| playback_response("spotify:track:abcdefghijkl0123456789", false)),
        );
        let (mut client, requests) = mock_client(&storage, responses);

        assert!(super::super::playback::play_song(
            &mut client,
            "spotify:track:0123456789abcdefghijkl",
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
        let _apartment = Apartment::new();
        let storage = TestStorage::new();
        let (mut client, requests) = mock_client(&storage, vec![devices_response()]);
        let checks = AtomicUsize::new(0);
        let cancelled = || checks.fetch_add(1, Ordering::SeqCst) != 0;

        assert!(super::super::playback::play_song(
            &mut client,
            "spotify:track:0123456789abcdefghijkl",
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
            .play("spotify:track:0123456789abcdefghijkl", &|| false)
            .unwrap_err();

        assert_eq!(error.status, SongStatus::ConnectRequired);
        assert!(error.message.contains("playback-state access"));
        assert_eq!(requests.lock().unwrap().len(), 1);
    }

    #[test]
    fn cancellation_during_token_refresh_prevents_retrying_the_play_command() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let _apartment = Apartment::new();
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
        let _apartment = Apartment::new();
        let storage = TestStorage::new();
        let devices = r#"{"devices":[{"id":null,"is_restricted":false},{"id":"restricted_pc","is_restricted":true},{"id":"bad&id=other","is_restricted":false}]}"#;
        let (mut client, requests) = mock_client(&storage, vec![response(200, devices, None)]);

        assert!(client
            .play("spotify:track:0123456789abcdefghijkl", &|| false)
            .is_err());

        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "GET");
    }

    #[test]
    fn the_final_confirmation_read_never_sends_a_retry_it_cannot_observe() {
        let _apartment = Apartment::new();
        let storage = TestStorage::new();
        let uri = "spotify:track:0123456789abcdefghijkl";
        let mut responses = vec![devices_response(), response(204, "", None)];
        responses.extend((0..4).map(|_| response(204, "", None)));
        responses.push(playback_response(uri, false));
        let (mut client, requests) = mock_client(&storage, responses);

        assert!(
            super::super::playback::play_song(&mut client, uri, "This PC", |_| {}, &|| false)
                .is_err()
        );

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
        let _apartment = Apartment::new();
        let storage = TestStorage::new();
        let (mut client, requests) = mock_client(&storage, vec![response(429, "", Some(3600))]);
        assert_eq!(
            client
                .play("spotify:track:0123456789abcdefghijkl", &|| false)
                .unwrap_err()
                .status,
            SongStatus::RateLimited
        );
        assert_eq!(
            client
                .play("spotify:track:0123456789abcdefghijkl", &|| false)
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
