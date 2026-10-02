//! PKCE authorization uses a loopback listener opened before the browser is launched.
use super::encoding::{challenge, random_secret, url_encode};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    thread,
    time::{Duration, Instant},
};
use windows::Win32::Foundation::HWND;

pub const REDIRECT_URI: &str = "http://127.0.0.1:43821/callback";
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(180);
const MAX_CALLBACK_BYTES: usize = 8192;
const AUTHORIZATION_SCOPES: &str = "user-modify-playback-state user-read-playback-state";

pub struct Authorization {
    pub code: String,
    pub verifier: String,
}

/// Called only after the person clicks Connect Spotify, never during startup or search.
pub fn authorize(client_id: &str, cancelled: &impl Fn() -> bool) -> Result<Authorization, String> {
    let listener = TcpListener::bind("127.0.0.1:43821").map_err(|_| {
        "Spotify's connection port is busy. Finish any other connection and try again."
    })?;
    listener
        .set_nonblocking(true)
        .map_err(|_| "Could not prepare the Spotify connection.")?;
    let verifier = random_secret()?;
    let state = random_secret()?;
    let url = format!("https://accounts.spotify.com/authorize?client_id={}&response_type=code&redirect_uri={}&scope={}&state={}&code_challenge_method=S256&code_challenge={}", url_encode(client_id), url_encode(REDIRECT_URI), url_encode(AUTHORIZATION_SCOPES), state, challenge(&verifier)?);
    if cancelled() {
        return Err("Spotify connection cancelled.".into());
    }
    crate::windows::execute_action::open_url(HWND::default(), &url)
        .map_err(|_| "Could not open Spotify's sign-in page.")?;
    let deadline = Instant::now() + CALLBACK_TIMEOUT;
    while Instant::now() < deadline && !cancelled() {
        match listener.accept() {
            Ok((mut stream, _)) => {
                if let Some(result) = handle_callback(&mut stream, &state) {
                    return result.map(|code| Authorization { code, verifier });
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(100))
            }
            Err(_) => return Err("Could not receive Spotify's sign-in response.".into()),
        }
    }
    Err(if cancelled() {
        "Spotify connection cancelled."
    } else {
        "Spotify sign-in timed out. Click Connect Spotify to try again."
    }
    .into())
}

fn handle_callback(stream: &mut TcpStream, state: &str) -> Option<Result<String, String>> {
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .ok()?;
    let mut request = Vec::new();
    let mut chunk = [0; 1024];
    while request.len() < MAX_CALLBACK_BYTES && !request.windows(4).any(|part| part == b"\r\n\r\n")
    {
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            break;
        }
        request.extend_from_slice(&chunk[..read]);
    }
    let parsed = std::str::from_utf8(&request)
        .ok()
        .and_then(|request| callback_code(request, state));
    let message = match &parsed {
        Some(Ok(_)) => "Spotify sign-in received. You can close this tab and return to Core.",
        Some(Err(_)) => "Spotify connection was declined. You can return to Core.",
        None => "This request does not match Core's Spotify connection.",
    };
    let status = if parsed.is_some() {
        "200 OK"
    } else {
        "400 Bad Request"
    };
    let response = format!("HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{message}", message.len());
    let _ = stream.write_all(response.as_bytes());
    parsed
}

/// Invalid paths and states are ignored so an unrelated request cannot consume the login.
fn callback_code(request: &str, expected_state: &str) -> Option<Result<String, String>> {
    let mut line = request.lines().next()?.split_whitespace();
    if line.next()? != "GET" {
        return None;
    }
    let target = line.next()?;
    let (path, query) = target.split_once('?')?;
    if path != "/callback" {
        return None;
    }
    let mut state = None;
    let mut code = None;
    let mut denied = false;
    for pair in query.split('&') {
        let (key, text) = pair.split_once('=')?;
        let text = url_decode(text)?;
        match key {
            "state" if state.is_none() => state = Some(text),
            "code" if code.is_none() => code = Some(text),
            "error" if !denied => denied = true,
            "state" | "code" | "error" => return None,
            _ => {}
        }
    }
    if state.as_deref()? != expected_state {
        return None;
    }
    if denied {
        return Some(Err(
            "Spotify connection was declined. Click Connect Spotify to try again.".into(),
        ));
    }
    let code = code.filter(|code| super::encoding::valid_token(code))?;
    Some(Ok(code))
}

fn url_decode(text: &str) -> Option<String> {
    let mut decoded = Vec::new();
    let mut bytes = text.bytes();
    while let Some(byte) = bytes.next() {
        decoded.push(match byte {
            b'%' => {
                let digits = [bytes.next()?, bytes.next()?];
                u8::from_str_radix(std::str::from_utf8(&digits).ok()?, 16).ok()?
            }
            b'+' => b' ',
            byte => byte,
        });
    }
    String::from_utf8(decoded).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn callback_requires_the_expected_path_state_and_unique_fields() {
        assert_eq!(
            callback_code(
                "GET /callback?code=abc%2Ddef&state=expected HTTP/1.1\r\n",
                "expected"
            ),
            Some(Ok("abc-def".into()))
        );
        for target in [
            "/other?code=abc&state=expected",
            "/callback?code=abc&state=wrong",
            "/callback?code=abc&state=expected&state=expected",
            "/callback?code=%0A&state=expected",
        ] {
            assert!(callback_code(&format!("GET {target} HTTP/1.1\r\n"), "expected").is_none());
        }
        assert!(callback_code(
            "GET /callback?error=access_denied&state=expected HTTP/1.1\r\n",
            "expected"
        )
        .unwrap()
        .is_err());
    }
}
