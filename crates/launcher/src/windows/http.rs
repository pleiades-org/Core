//! Minimal HTTP GET over WinHTTP for background workers: website icons and exchange rates.
//! No cookies or credentials are sent, responses are size-limited, and slow servers time out.
use std::{sync::OnceLock, time::Instant};
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::{E_FAIL, WIN32_ERROR},
        Networking::WinHttp::*,
    },
};

/// Resolve, connect and send limits; receiving the body gets a little longer.
const CONNECT_TIMEOUT_MS: i32 = 3_000;
const RECEIVE_TIMEOUT_MS: i32 = 5_000;
const CHUNK_BYTES: usize = 16 * 1024;
pub const STATUS_OK: u32 = 200;

#[derive(Clone, Copy)]
pub struct Request<'a> {
    pub secure: bool,
    pub host: &'a str,
    pub port: u16,
    pub path: &'a str,
    pub max_bytes: usize,
}

pub struct Response {
    pub status: u32,
    pub location: Option<String>,
    /// Size-limited response bytes. `get` clears bodies for unsuccessful responses.
    pub body: Vec<u8>,
    pub retry_after: Option<u32>,
}

/// Authenticated API calls. Callers supply validated headers; redirects are never followed.
pub struct Exchange<'a> {
    pub endpoint: Request<'a>,
    pub method: &'static str,
    pub headers: &'a str,
    pub body: &'a [u8],
}

struct Internet(*mut core::ffi::c_void);
impl Internet {
    fn new(handle: *mut core::ffi::c_void) -> windows::core::Result<Self> {
        if handle.is_null() {
            Err(windows::core::Error::from_win32())
        } else {
            Ok(Self(handle))
        }
    }
}
impl Drop for Internet {
    fn drop(&mut self) {
        unsafe {
            let _ = WinHttpCloseHandle(self.0);
        }
    }
}

/// One session for the process, so proxy detection and open connections are reused.
/// Synchronous WinHTTP handles may be used from several threads at once.
struct Session(Internet);
unsafe impl Send for Session {}
unsafe impl Sync for Session {}
static SESSION: OnceLock<Session> = OnceLock::new();

/// Opens the session on first use; a failed open is retried by the next request.
fn session() -> windows::core::Result<&'static Internet> {
    if let Some(session) = SESSION.get() {
        return Ok(&session.0);
    }
    let opened = Internet::new(unsafe {
        WinHttpOpen(
            w!("Core/2"),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        )
    })?;
    // If another thread opened one first, that one is kept and this one closes.
    Ok(&SESSION.get_or_init(|| Session(opened)).0)
}

/// Blocking GET; call only from a background thread.
pub fn get(request: Request, deadline: Instant) -> windows::core::Result<Response> {
    let mut response = exchange(
        Exchange {
            endpoint: request,
            method: "GET",
            headers: "",
            body: &[],
        },
        deadline,
    )?;
    if response.status != STATUS_OK {
        response.body.clear();
    }
    Ok(response)
}

pub fn exchange(exchange: Exchange<'_>, deadline: Instant) -> windows::core::Result<Response> {
    let request = exchange.endpoint;
    unsafe {
        // Timeouts are set on each request handle, never on the shared session.
        let session = session()?;
        let host: Vec<u16> = request.host.encode_utf16().chain(Some(0)).collect();
        let connection = Internet::new(WinHttpConnect(
            session.0,
            PCWSTR(host.as_ptr()),
            request.port,
            0,
        ))?;
        let path: Vec<u16> = request.path.encode_utf16().chain(Some(0)).collect();
        let handle = Internet::new(WinHttpOpenRequest(
            connection.0,
            PCWSTR(super::wide(exchange.method).as_ptr()),
            PCWSTR(path.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            if request.secure {
                WINHTTP_FLAG_SECURE
            } else {
                WINHTTP_OPEN_REQUEST_FLAGS(0)
            },
        ))?;
        let disabled = (WINHTTP_DISABLE_COOKIES | WINHTTP_DISABLE_AUTHENTICATION).to_le_bytes();
        WinHttpSetOption(
            Some(handle.0),
            WINHTTP_OPTION_DISABLE_FEATURE,
            Some(&disabled),
        )?;
        WinHttpSetOption(
            Some(handle.0),
            WINHTTP_OPTION_REDIRECT_POLICY,
            Some(&WINHTTP_OPTION_REDIRECT_POLICY_NEVER.to_le_bytes()),
        )?;
        set_timeouts(handle.0, deadline)?;
        let headers = super::wide(exchange.headers);
        WinHttpSendRequest(
            handle.0,
            Some(&headers[..headers.len() - 1]),
            (!exchange.body.is_empty()).then_some(exchange.body.as_ptr().cast()),
            exchange.body.len() as u32,
            exchange.body.len() as u32,
            0,
        )?;
        set_timeouts(handle.0, deadline)?;
        WinHttpReceiveResponse(handle.0, std::ptr::null_mut())?;
        remaining_timeout(deadline, Instant::now())?;
        let mut status = 0_u32;
        let mut length = std::mem::size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            handle.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some((&mut status as *mut u32).cast()),
            &mut length,
            std::ptr::null_mut(),
        )?;
        let location = if matches!(status, 301 | 302 | 303 | 307 | 308) {
            header(handle.0, WINHTTP_QUERY_LOCATION)?
        } else {
            None
        };
        let retry_after =
            header(handle.0, WINHTTP_QUERY_RETRY_AFTER)?.and_then(|text| text.parse().ok());
        let body = if status != 204 && !(300..400).contains(&status) {
            let length = header(handle.0, WINHTTP_QUERY_CONTENT_LENGTH)?;
            validate_length(length.as_deref(), request.max_bytes)?;
            read_body(handle.0, request.max_bytes, deadline)?
        } else {
            Vec::new()
        };
        Ok(Response {
            status,
            location,
            body,
            retry_after,
        })
    }
}

unsafe fn read_body(
    request: *mut core::ffi::c_void,
    max_bytes: usize,
    deadline: Instant,
) -> windows::core::Result<Vec<u8>> {
    let mut body = Vec::new();
    let mut chunk = [0_u8; CHUNK_BYTES];
    loop {
        set_timeouts(request, deadline)?;
        let mut available = 0_u32;
        WinHttpQueryDataAvailable(request, &mut available)?;
        remaining_timeout(deadline, Instant::now())?;
        if available == 0 {
            return Ok(body);
        }
        if available as usize > max_bytes.saturating_sub(body.len()) {
            return Err(windows::core::Error::new(
                E_FAIL,
                "response exceeds size limit",
            ));
        }
        set_timeouts(request, deadline)?;
        let mut read = 0_u32;
        unsafe {
            WinHttpReadData(
                request,
                chunk.as_mut_ptr().cast(),
                available.min(chunk.len() as u32),
                &mut read,
            )?;
        }
        remaining_timeout(deadline, Instant::now())?;
        if read == 0 {
            return Ok(body);
        }
        body.extend_from_slice(&chunk[..read as usize]);
        if body.len() > max_bytes {
            return Err(windows::core::Error::new(
                E_FAIL,
                format!("response exceeds {max_bytes} bytes"),
            ));
        }
    }
}

fn remaining_timeout(deadline: Instant, now: Instant) -> windows::core::Result<i32> {
    let remaining = deadline
        .checked_duration_since(now)
        .filter(|duration| !duration.is_zero())
        .ok_or_else(|| {
            windows::core::Error::new(E_FAIL, "HTTP request exceeded its total deadline")
        })?;
    Ok(remaining.as_millis().clamp(1, RECEIVE_TIMEOUT_MS as u128) as i32)
}

unsafe fn set_timeouts(
    handle: *mut core::ffi::c_void,
    deadline: Instant,
) -> windows::core::Result<()> {
    let receive = remaining_timeout(deadline, Instant::now())?;
    let connect = receive.min(CONNECT_TIMEOUT_MS);
    WinHttpSetTimeouts(handle, connect, connect, connect, receive)
}

unsafe fn header(
    request: *mut core::ffi::c_void,
    query: u32,
) -> windows::core::Result<Option<String>> {
    let mut buffer = [0_u16; 4096];
    let mut length = std::mem::size_of_val(&buffer) as u32;
    match WinHttpQueryHeaders(
        request,
        query,
        PCWSTR::null(),
        Some(buffer.as_mut_ptr().cast()),
        &mut length,
        std::ptr::null_mut(),
    ) {
        Ok(()) => {
            let text = String::from_utf16(&buffer[..length as usize / 2])
                .map_err(|_| windows::core::Error::new(E_FAIL, "invalid HTTP header encoding"))?;
            Ok(Some(text.trim_end_matches('\0').to_owned()))
        }
        Err(error) if error.code() == WIN32_ERROR(ERROR_WINHTTP_HEADER_NOT_FOUND).to_hresult() => {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn validate_length(length: Option<&str>, max_bytes: usize) -> windows::core::Result<()> {
    let Some(length) = length else { return Ok(()) };
    let length = length
        .parse::<u64>()
        .map_err(|_| windows::core::Error::new(E_FAIL, "invalid Content-Length"))?;
    if length > max_bytes as u64 {
        return Err(windows::core::Error::new(
            E_FAIL,
            "declared response exceeds size limit",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn deadlines_expire_at_the_boundary_and_bound_each_wait() {
        let now = Instant::now();
        assert_eq!(
            remaining_timeout(now + Duration::from_secs(30), now).unwrap(),
            RECEIVE_TIMEOUT_MS
        );
        assert_eq!(
            remaining_timeout(now + Duration::from_millis(12), now).unwrap(),
            12
        );
        assert!(remaining_timeout(now, now).is_err());
        assert!(remaining_timeout(now, now + Duration::from_nanos(1)).is_err());
    }

    #[test]
    fn a_trickling_body_cannot_extend_the_total_deadline() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            thread,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        let server = thread::spawn(move || {
            let started = Instant::now();
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(started.elapsed() < Duration::from_secs(2));
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("could not accept test request: {error}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = [0; 4096];
            assert!(stream.read(&mut request).unwrap() > 0);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\n")
                .unwrap();
            for _ in 0..100 {
                if stream.write_all(b"x").is_err() {
                    return;
                }
                thread::sleep(Duration::from_millis(20));
            }
        });
        let started = Instant::now();
        let result = get(
            Request {
                secure: false,
                host: "127.0.0.1",
                port,
                path: "/",
                max_bytes: 100,
            },
            started + Duration::from_millis(250),
        );
        let elapsed = started.elapsed();
        server.join().unwrap();
        assert!(result.is_err());
        assert!(
            elapsed < Duration::from_secs(1),
            "request ran for {elapsed:?}"
        );
    }

    #[test]
    fn every_request_shares_one_session() {
        let first = session().unwrap().0 as usize;
        let other_thread = std::thread::spawn(|| session().unwrap().0 as usize)
            .join()
            .unwrap();
        assert_ne!(first, 0);
        assert_eq!(first, other_thread);
    }

    #[test]
    fn declared_lengths_are_checked_before_reading() {
        assert!(validate_length(None, 10).is_ok());
        assert!(validate_length(Some("10"), 10).is_ok());
        for length in ["11", "18446744073709551615", "-1", "invalid"] {
            assert!(validate_length(Some(length), 10).is_err());
        }
    }

    #[test]
    fn api_exchange_sends_method_headers_and_body_and_preserves_rate_limit_details() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            thread,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let body = br#"{"uris":["spotify:track:0123456789abcdefghijkl"],"position_ms":0}"#;
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 1024];
            loop {
                let read = stream.read(&mut chunk).unwrap();
                assert_ne!(read, 0);
                request.extend_from_slice(&chunk[..read]);
                if let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                    if request.len() >= header_end + 4 + body.len() {
                        break;
                    }
                }
                assert!(request.len() < 8192);
            }
            stream.write_all(b"HTTP/1.1 429 Too Many Requests\r\nRetry-After: 120\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").unwrap();
            String::from_utf8(request).unwrap()
        });
        let response = exchange(
            Exchange {
                endpoint: Request {
                    secure: false,
                    host: "127.0.0.1",
                    port,
                    path: "/v1/me/player/play",
                    max_bytes: 1024,
                },
                method: "PUT",
                headers: "Authorization: Bearer test-only\r\nContent-Type: application/json\r\n",
                body,
            },
            Instant::now() + Duration::from_secs(4),
        )
        .unwrap();
        let request = server.join().unwrap();
        assert!(request.starts_with("PUT /v1/me/player/play HTTP/1.1"));
        assert!(request.contains("Authorization: Bearer test-only\r\n"));
        let (headers, received_body) = request.split_once("\r\n\r\n").unwrap();
        let content_type = headers.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("Content-Type")
                .then_some(value.trim())
        });
        let content_length = headers.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("Content-Length")
                .then_some(value.trim())
        });
        assert_eq!(content_type, Some("application/json"));
        assert_eq!(
            content_length.unwrap().parse::<usize>().unwrap(),
            body.len()
        );
        assert_eq!(received_body.as_bytes(), body);
        assert_eq!(response.status, 429);
        assert_eq!(response.retry_after, Some(120));
        assert_eq!(response.body, b"{}");
    }
}
