//! Minimal HTTP GET over WinHTTP for background workers: website icons and exchange rates.
//! No cookies or credentials are sent, responses are size-limited, and slow servers time out.
use windows::{
    core::{w, PCWSTR},
    Win32::{Foundation::E_FAIL, Networking::WinHttp::*},
};

/// Resolve, connect and send limits; receiving the body gets a little longer.
const CONNECT_TIMEOUT_MS: i32 = 3_000;
const RECEIVE_TIMEOUT_MS: i32 = 5_000;
const CHUNK_BYTES: usize = 16 * 1024;
pub const STATUS_OK: u32 = 200;

pub struct Request<'a> {
    pub secure: bool,
    pub host: &'a str,
    pub port: u16,
    pub path: &'a str,
    pub max_bytes: usize,
}

pub struct Response {
    pub status: u32,
    /// Empty unless the status is 200.
    pub body: Vec<u8>,
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

/// Blocking GET; call only from a background thread.
pub fn get(request: Request) -> windows::core::Result<Response> {
    unsafe {
        let session = Internet::new(WinHttpOpen(
            w!("Core/2"),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        ))?;
        WinHttpSetTimeouts(
            session.0,
            CONNECT_TIMEOUT_MS,
            CONNECT_TIMEOUT_MS,
            CONNECT_TIMEOUT_MS,
            RECEIVE_TIMEOUT_MS,
        )?;
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
            w!("GET"),
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
        WinHttpSendRequest(handle.0, None, None, 0, 0, 0)?;
        WinHttpReceiveResponse(handle.0, std::ptr::null_mut())?;
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
        let body = if status == STATUS_OK {
            read_body(handle.0, request.max_bytes)?
        } else {
            Vec::new()
        };
        Ok(Response { status, body })
    }
}

unsafe fn read_body(
    request: *mut core::ffi::c_void,
    max_bytes: usize,
) -> windows::core::Result<Vec<u8>> {
    let mut body = Vec::new();
    let mut chunk = [0_u8; CHUNK_BYTES];
    loop {
        let mut read = 0_u32;
        unsafe {
            WinHttpReadData(
                request,
                chunk.as_mut_ptr().cast(),
                chunk.len() as u32,
                &mut read,
            )?;
        }
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
