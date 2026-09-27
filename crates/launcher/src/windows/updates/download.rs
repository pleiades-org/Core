//! Explicit GitHub redirects preserve the shared HTTP client's no-redirect default.
use super::{super::http, UpdateError};
use std::time::{Duration, Instant};

const MAX_REDIRECTS: usize = 5;
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(90);

pub fn get(path: &str, max_bytes: usize) -> Result<Vec<u8>, UpdateError> {
    let deadline = Instant::now() + DOWNLOAD_TIMEOUT;
    let mut host = String::from("github.com");
    let mut path = path.to_owned();
    for _ in 0..=MAX_REDIRECTS {
        let response = http::get(
            http::Request {
                secure: true,
                host: &host,
                port: 443,
                path: &path,
                max_bytes,
            },
            deadline,
        )
        .map_err(UpdateError::Network)?;
        match response.status {
            http::STATUS_OK => return Ok(response.body),
            301 | 302 | 303 | 307 | 308 => {
                (host, path) = redirect(
                    &host,
                    response
                        .location
                        .as_deref()
                        .ok_or(UpdateError::UnexpectedResponse(response.status))?,
                )?;
            }
            status => return Err(UpdateError::UnexpectedResponse(status)),
        }
    }
    Err(UpdateError::UnsafeRedirect)
}

fn redirect(current_host: &str, location: &str) -> Result<(String, String), UpdateError> {
    if location.contains(['\\', '\r', '\n', '#']) || location.chars().any(char::is_whitespace) {
        return Err(UpdateError::UnsafeRedirect);
    }
    if location.starts_with('/') && !location.starts_with("//") {
        return Ok((current_host.into(), location.into()));
    }
    let (host, path) = location
        .strip_prefix("https://")
        .and_then(|rest| rest.split_once('/'))
        .ok_or(UpdateError::UnsafeRedirect)?;
    if ![
        "github.com",
        "release-assets.githubusercontent.com",
        "objects.githubusercontent.com",
    ]
    .contains(&host)
    {
        return Err(UpdateError::UnsafeRedirect);
    }
    Ok((host.into(), format!("/{path}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn redirects_only_accept_github_https_assets() {
        for url in [
            "https://github.com/a",
            "https://release-assets.githubusercontent.com/a?token=x",
            "/a",
        ] {
            assert!(redirect("github.com", url).is_ok());
        }
        for url in [
            "http://github.com/a",
            "//evil.test/a",
            "https://github.com@evil.test/a",
            "https://github.com.evil.test/a",
            "https://localhost/a",
            "https://github.com:443/a",
            "/a\rb",
        ] {
            assert!(redirect("github.com", url).is_err(), "{url}");
        }
    }
}
