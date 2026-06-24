use crate::{
    command::{CommandAction, CommandCategory, CommandResult},
    paths::cache_dir,
    search_text::normalize_search_text,
};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use chrono::{DateTime, TimeZone, Utc};
use cron::Schedule;
use image::Luma;
use jsonpath_rust::JsonPath;
use md5::{Digest as Md5Digest, Md5};
use qrcode::QrCode;
use sha2::Sha256;
use std::{fs, str::FromStr, time::{SystemTime, UNIX_EPOCH}};
use uuid::Uuid;

pub fn search_dev_tools(search_text: &str) -> Vec<CommandResult> {
    let normalized_search_text = normalize_search_text(search_text);
    if normalized_search_text.is_empty() {
        return dev_tool_catalog();
    }

    let mut results = Vec::new();
    results.extend(execute_dev_tool_query(search_text));
    results.extend(
        dev_tool_catalog()
            .into_iter()
            .filter(|result| {
                normalize_search_text(&result.title).contains(&normalized_search_text)
                    || normalize_search_text(&result.subtitle).contains(&normalized_search_text)
            }),
    );
    results
}

pub fn search_inline(query: &str) -> Vec<CommandResult> {
    execute_dev_tool_query(query)
}

fn dev_tool_catalog() -> Vec<CommandResult> {
    vec![
        hint_result("Generate UUID v4", "uuid", "uuid", 86),
        hint_result("SHA256 hash", "sha256 <text>", "sha256 ", 84),
        hint_result("MD5 hash", "md5 <text>", "md5 ", 83),
        hint_result("Base64 encode", "base64 encode <text>", "base64 encode ", 82),
        hint_result("Base64 decode", "base64 decode <text>", "base64 decode ", 82),
        hint_result("Format JSON", "json format {...}", "json format ", 81),
        hint_result("URL encode", "url encode <text>", "url encode ", 80),
        hint_result("URL decode", "url decode <text>", "url decode ", 80),
        hint_result("Random password", "password [length]", "password ", 79),
        hint_result("Lorem ipsum", "lorem [word count]", "lorem ", 78),
        hint_result("Unix timestamp now", "unix timestamp", "unix timestamp", 77),
        hint_result(
            "Convert timestamp to date",
            "timestamp <seconds> to date",
            "timestamp ",
            76,
        ),
        hint_result(
            "JSONPath extract",
            "jsonpath $.key {\"key\":\"value\"}",
            "jsonpath ",
            75,
        ),
        hint_result("Cron next runs", "cron */5 * * * *", "cron ", 74),
        hint_result("Generate QR code", "qr https://example.com", "qr ", 73),
    ]
}

fn hint_result(title: &str, subtitle: &str, copy_text: &str, confidence: u8) -> CommandResult {
    CommandResult::copyable_feature(
        title,
        subtitle,
        copy_text,
        CommandCategory::DevTools,
        confidence,
    )
}

fn execute_dev_tool_query(query: &str) -> Vec<CommandResult> {
    let trimmed_query = query.trim();
    if trimmed_query.is_empty() {
        return Vec::new();
    }

    let normalized_query = normalize_search_text(trimmed_query);

    if let Some(result) = try_uuid(trimmed_query, &normalized_query) {
        return vec![result];
    }
    if let Some(result) = try_sha256(trimmed_query, &normalized_query) {
        return vec![result];
    }
    if let Some(result) = try_md5(trimmed_query, &normalized_query) {
        return vec![result];
    }
    if let Some(result) = try_base64(trimmed_query, &normalized_query) {
        return vec![result];
    }
    if let Some(result) = try_json_format(trimmed_query, &normalized_query) {
        return vec![result];
    }
    if let Some(result) = try_url_encode_decode(trimmed_query, &normalized_query) {
        return vec![result];
    }
    if let Some(result) = try_password(trimmed_query, &normalized_query) {
        return vec![result];
    }
    if let Some(result) = try_lorem(trimmed_query, &normalized_query) {
        return vec![result];
    }
    if let Some(result) = try_unix_timestamp(trimmed_query, &normalized_query) {
        return vec![result];
    }
    if let Some(result) = try_timestamp_to_date(trimmed_query, &normalized_query) {
        return vec![result];
    }
    if let Some(result) = try_jsonpath(trimmed_query, &normalized_query) {
        return vec![result];
    }
    if let Some(result) = try_cron(trimmed_query, &normalized_query) {
        return vec![result];
    }
    if let Some(results) = try_qr(trimmed_query, &normalized_query) {
        return results;
    }

    Vec::new()
}

fn try_uuid(query: &str, normalized_query: &str) -> Option<CommandResult> {
    if normalized_query == "uuid"
        || normalized_query == "uuid v4"
        || normalized_query == "generate uuid"
        || normalized_query.starts_with("uuid generate")
    {
        let value = Uuid::new_v4().to_string();
        return Some(copy_result(
            "UUID v4",
            "Generated unique identifier",
            value,
            92,
        ));
    }

    let _ = query;
    None
}

fn try_sha256(query: &str, normalized_query: &str) -> Option<CommandResult> {
    let input = normalized_query
        .strip_prefix("sha256 ")
        .or_else(|| normalized_query.strip_prefix("hash sha256 "))
        .or_else(|| normalized_query.strip_prefix("@dev sha256 "))
        .or_else(|| normalized_query.strip_prefix("@util sha256 "))?;

    if input.is_empty() {
        return None;
    }

    let original_input = extract_payload_after_prefix(query, &["sha256", "hash sha256", "@dev sha256", "@util sha256"]);
    let digest = format!("{:x}", Sha256::digest(original_input.as_bytes()));
    Some(copy_result(
        "SHA256",
        &truncate_preview(&original_input, 48),
        digest,
        90,
    ))
}

fn try_md5(query: &str, normalized_query: &str) -> Option<CommandResult> {
    let input = normalized_query
        .strip_prefix("md5 ")
        .or_else(|| normalized_query.strip_prefix("hash md5 "))
        .or_else(|| normalized_query.strip_prefix("@dev md5 "))
        .or_else(|| normalized_query.strip_prefix("@util md5 "))?;

    if input.is_empty() {
        return None;
    }

    let original_input = extract_payload_after_prefix(query, &["md5", "hash md5", "@dev md5", "@util md5"]);
    let digest = format!("{:x}", Md5::digest(original_input.as_bytes()));
    Some(copy_result(
        "MD5",
        &truncate_preview(&original_input, 48),
        digest,
        89,
    ))
}

fn try_base64(query: &str, normalized_query: &str) -> Option<CommandResult> {
    if let Some(input) = normalized_query.strip_prefix("base64 encode ") {
        if input.is_empty() {
            return None;
        }
        let original_input =
            extract_payload_after_prefix(query, &["base64 encode", "@dev base64 encode", "@util base64 encode"]);
        let encoded = BASE64_STANDARD.encode(original_input.as_bytes());
        return Some(copy_result(
            "Base64 encoded",
            &truncate_preview(&original_input, 48),
            encoded,
            88,
        ));
    }

    if let Some(input) = normalized_query.strip_prefix("base64 decode ") {
        if input.is_empty() {
            return None;
        }
        let original_input =
            extract_payload_after_prefix(query, &["base64 decode", "@dev base64 decode", "@util base64 decode"]);
        let decoded = BASE64_STANDARD
            .decode(original_input.trim())
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())?;
        return Some(copy_result(
            "Base64 decoded",
            &truncate_preview(&original_input, 48),
            decoded,
            88,
        ));
    }

    None
}

fn try_json_format(query: &str, normalized_query: &str) -> Option<CommandResult> {
    let json_text = normalized_query
        .strip_prefix("json format ")
        .or_else(|| normalized_query.strip_prefix("json prettify "))
        .or_else(|| normalized_query.strip_prefix("prettify json "))
        .map(|_| extract_payload_after_prefix(query, &["json format", "json prettify", "prettify json"]))
        .or_else(|| {
            let trimmed = query.trim();
            if trimmed.starts_with('{') || trimmed.starts_with('[') {
                Some(trimmed.to_string())
            } else {
                None
            }
        })?;

    let value: serde_json::Value = serde_json::from_str(&json_text).ok()?;
    let formatted = serde_json::to_string_pretty(&value).ok()?;
    Some(copy_result(
        "Formatted JSON",
        &truncate_preview(&json_text, 48),
        formatted,
        87,
    ))
}

fn try_url_encode_decode(query: &str, normalized_query: &str) -> Option<CommandResult> {
    if let Some(input) = normalized_query.strip_prefix("url encode ") {
        if input.is_empty() {
            return None;
        }
        let original_input =
            extract_payload_after_prefix(query, &["url encode", "@dev url encode", "@util url encode"]);
        let encoded = url_encode(&original_input);
        return Some(copy_result(
            "URL encoded",
            &truncate_preview(&original_input, 48),
            encoded,
            86,
        ));
    }

    if let Some(input) = normalized_query.strip_prefix("url decode ") {
        if input.is_empty() {
            return None;
        }
        let original_input =
            extract_payload_after_prefix(query, &["url decode", "@dev url decode", "@util url decode"]);
        let decoded = url_decode(&original_input)?;
        return Some(copy_result(
            "URL decoded",
            &truncate_preview(&original_input, 48),
            decoded,
            86,
        ));
    }

    None
}

fn try_password(query: &str, normalized_query: &str) -> Option<CommandResult> {
    let length = if normalized_query == "password" || normalized_query == "random password" {
        20
    } else if let Some(length_text) = normalized_query
        .strip_prefix("password ")
        .or_else(|| normalized_query.strip_prefix("random password "))
    {
        length_text.parse().ok()?
    } else {
        return None;
    };

    if !(8..=128).contains(&length) {
        return None;
    }

    let password = generate_password(length);
    let _ = query;
    Some(copy_result(
        format!("Random password ({length} chars)"),
        "Copy and store in a password manager",
        password,
        85,
    ))
}

fn try_lorem(query: &str, normalized_query: &str) -> Option<CommandResult> {
    let word_count = if normalized_query == "lorem" || normalized_query == "lorem ipsum" {
        50
    } else if let Some(count_text) = normalized_query
        .strip_prefix("lorem ")
        .or_else(|| normalized_query.strip_prefix("lorem ipsum "))
    {
        count_text.parse().ok()?
    } else {
        return None;
    };

    if !(1..=500).contains(&word_count) {
        return None;
    }

    let text = generate_lorem_ipsum(word_count);
    let _ = query;
    Some(copy_result(
        format!("Lorem ipsum ({word_count} words)"),
        "Placeholder text",
        text,
        84,
    ))
}

fn try_unix_timestamp(query: &str, normalized_query: &str) -> Option<CommandResult> {
    if matches!(
        normalized_query,
        "unix" | "unix timestamp" | "timestamp now" | "unix time" | "unix now"
    ) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_secs();
        let _ = query;
        return Some(copy_result(
            "Unix timestamp (now)",
            Utc::now().to_rfc3339(),
            now.to_string(),
            83,
        ));
    }

    None
}

fn try_timestamp_to_date(query: &str, normalized_query: &str) -> Option<CommandResult> {
    let digits = normalized_query
        .strip_prefix("timestamp ")
        .and_then(|rest| rest.strip_suffix(" to date"))
        .or_else(|| {
            normalized_query
                .strip_prefix("unix ")
                .and_then(|rest| rest.strip_suffix(" to date"))
        })
        .or_else(|| normalized_query.strip_suffix(" to date"))
        .map(str::trim)
        .filter(|value| value.chars().all(|character| character.is_ascii_digit()))?;

    let seconds: i64 = digits.parse().ok()?;
    let datetime = Utc.timestamp_opt(seconds, 0).single()?;
    let formatted = datetime.to_rfc3339();
    let _ = query;
    Some(copy_result(
        "Timestamp to date",
        digits,
        formatted,
        82,
    ))
}

fn try_jsonpath(query: &str, normalized_query: &str) -> Option<CommandResult> {
    let rest = normalized_query
        .strip_prefix("jsonpath ")
        .or_else(|| normalized_query.strip_prefix("@dev jsonpath "))?;
    let (path_text, json_text) = split_jsonpath_query(query, rest)?;

    let value = extract_jsonpath(&json_text, &path_text)?;
    Some(copy_result(
        "JSONPath result",
        &truncate_preview(&path_text, 48),
        value,
        88,
    ))
}

pub fn extract_jsonpath(json_text: &str, path_text: &str) -> Option<String> {
    let json_value: serde_json::Value = serde_json::from_str(json_text).ok()?;
    let path = JsonPath::from_str(path_text).ok()?;
    let results = path.find(&json_value);

    match results {
        serde_json::Value::Null => None,
        serde_json::Value::Array(items) if items.is_empty() => None,
        serde_json::Value::Array(items) if items.len() == 1 => serde_json::to_string(&items[0])
            .ok()
            .map(|value| value.trim_matches('"').to_string()),
        serde_json::Value::Array(items) => {
            serde_json::to_string_pretty(&serde_json::Value::Array(items)).ok()
        }
        single => serde_json::to_string(&single)
            .ok()
            .map(|value| value.trim_matches('"').to_string()),
    }
}

fn split_jsonpath_query(query: &str, normalized_rest: &str) -> Option<(String, String)> {
    let trimmed = query.trim();
    let lower = trimmed.to_lowercase();
    let prefix = if lower.starts_with("@dev jsonpath ") {
        "@dev jsonpath "
    } else if lower.starts_with("jsonpath ") {
        "jsonpath "
    } else {
        return None;
    };

    let payload = &trimmed[prefix.len()..];
    let json_start = payload.find('{').or_else(|| payload.find('['))?;
    let path_text = payload[..json_start].trim().to_string();
    let json_text = payload[json_start..].trim().to_string();

    if path_text.is_empty() || json_text.is_empty() {
        let _ = normalized_rest;
        return None;
    }

    Some((path_text, json_text))
}

fn try_cron(query: &str, normalized_query: &str) -> Option<CommandResult> {
    let expression = normalized_query
        .strip_prefix("cron ")
        .or_else(|| normalized_query.strip_prefix("@dev cron "))?;

    if expression.is_empty() {
        return None;
    }

    let original_expression =
        extract_payload_after_prefix(query, &["cron", "@dev cron"]);
    let next_runs = cron_next_runs(&original_expression, 5)?;
    Some(copy_result(
        format!("Cron: {original_expression}"),
        "Next 5 scheduled run times (UTC)",
        next_runs,
        87,
    ))
}

pub fn cron_next_runs(expression: &str, count: usize) -> Option<String> {
    let schedule = Schedule::from_str(expression)
        .or_else(|_| Schedule::from_str(&format!("0 {expression}")))
        .ok()?;
    let runs = schedule
        .upcoming(Utc)
        .take(count)
        .map(|datetime: DateTime<Utc>| datetime.to_rfc3339())
        .collect::<Vec<_>>();

    if runs.is_empty() {
        return None;
    }

    Some(runs.join("\n"))
}

fn try_qr(query: &str, normalized_query: &str) -> Option<Vec<CommandResult>> {
    let content = normalized_query
        .strip_prefix("qr ")
        .or_else(|| normalized_query.strip_prefix("@dev qr "))?;

    if content.is_empty() {
        return None;
    }

    let original_content = extract_payload_after_prefix(query, &["qr", "@dev qr"]);
    let file_path = generate_qr_png(&original_content)?;
    let path_display = file_path.display().to_string();

    let mut results = vec![copy_result(
        "QR code generated",
        &path_display,
        path_display.clone(),
        86,
    )];

    results.push(CommandResult {
        title: "Open QR folder".to_string(),
        subtitle: path_display.clone(),
        copy_text: path_display,
        explanation: None,
        icon_path: None,
        calculation_display: None,
        category: CommandCategory::DevTools,
        action: CommandAction::RunProgram {
            program: "explorer.exe".to_string(),
            arguments: vec![file_path
                .parent()
                .map(|parent| parent.display().to_string())
                .unwrap_or_else(|| ".".to_string())],
        },
        confidence: 84,
    });

    Some(results)
}

pub fn generate_qr_png(content: &str) -> Option<std::path::PathBuf> {
    let code = QrCode::new(content.as_bytes()).ok()?;
    let image = code.render::<Luma<u8>>().min_dimensions(256, 256).build();
    let cache = cache_dir();
    fs::create_dir_all(&cache).ok()?;
    let file_path = cache.join(format!("qr-{}.png", Uuid::new_v4()));
    image.save(&file_path).ok()?;
    Some(file_path)
}

fn copy_result(
    title: impl Into<String>,
    subtitle: impl Into<String>,
    copy_text: impl Into<String>,
    confidence: u8,
) -> CommandResult {
    CommandResult::copyable_feature(title, subtitle, copy_text, CommandCategory::DevTools, confidence)
}

fn extract_payload_after_prefix(query: &str, prefixes: &[&str]) -> String {
    let trimmed = query.trim();
    let lower = trimmed.to_lowercase();
    for prefix in prefixes {
        let normalized_prefix = prefix.to_lowercase();
        if let Some(rest) = lower.strip_prefix(&normalized_prefix) {
            return trimmed[trimmed.len() - rest.len()..].trim().to_string();
        }
    }
    trimmed.to_string()
}

fn truncate_preview(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }

    format!(
        "{}...",
        value.chars().take(max_chars.saturating_sub(3)).collect::<String>()
    )
}

pub fn url_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            b' ' => encoded.push('+'),
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

pub fn url_decode(value: &str) -> Option<String> {
    let mut decoded = Vec::new();
    let mut chars = value.chars().peekable();

    while let Some(character) = chars.next() {
        match character {
            '+' => decoded.push(b' '),
            '%' => {
                let high = chars.next()?;
                let low = chars.next()?;
                let hex = format!("{high}{low}");
                let byte = u8::from_str_radix(&hex, 16).ok()?;
                decoded.push(byte);
            }
            _ if character.is_ascii() => decoded.push(character as u8),
            _ => {
                let mut buffer = [0u8; 4];
                let encoded = character.encode_utf8(&mut buffer);
                decoded.extend_from_slice(encoded.as_bytes());
            }
        }
    }

    String::from_utf8(decoded).ok()
}

pub fn generate_password(length: usize) -> String {
    const CHARSET: &[u8] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*-_=+";
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let mut state = seed;

    (0..length)
        .map(|index| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1)
                .wrapping_add(index as u128);
            let charset_index = (state % CHARSET.len() as u128) as usize;
            CHARSET[charset_index] as char
        })
        .collect()
}

pub fn generate_lorem_ipsum(word_count: usize) -> String {
    const WORDS: &[&str] = &[
        "lorem", "ipsum", "dolor", "sit", "amet", "consectetur", "adipiscing", "elit", "sed", "do",
        "eiusmod", "tempor", "incididunt", "ut", "labore", "et", "dolore", "magna", "aliqua",
        "enim", "ad", "minim", "veniam", "quis", "nostrud", "exercitation", "ullamco", "laboris",
        "nisi", "aliquip", "ex", "ea", "commodo", "consequat", "duis", "aute", "irure", "in",
        "reprehenderit", "voluptate", "velit", "esse", "cillum", "fugiat", "nulla", "pariatur",
    ];

    WORDS
        .iter()
        .cycle()
        .take(word_count)
        .copied()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn format_json_text(json_text: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(json_text).ok()?;
    serde_json::to_string_pretty(&value).ok()
}

pub fn sha256_hex(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

pub fn md5_hex(value: &str) -> String {
    format!("{:x}", Md5::digest(value.as_bytes()))
}

pub fn timestamp_to_rfc3339(seconds: i64) -> Option<String> {
    Utc.timestamp_opt(seconds, 0)
        .single()
        .map(|datetime| datetime.to_rfc3339())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_text() {
        assert_eq!(
            sha256_hex("hello"),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert_eq!(md5_hex("hello"), "5d41402abc4b2a76b9719d911017c592");
    }

    #[test]
    fn base64_round_trip_via_query() {
        let results = execute_dev_tool_query("base64 encode hello");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].copy_text, "aGVsbG8=");

        let decoded = execute_dev_tool_query("base64 decode aGVsbG8=");
        assert_eq!(decoded[0].copy_text, "hello");
    }

    #[test]
    fn formats_json() {
        let results = execute_dev_tool_query(r#"json format {"a":1}"#);
        assert!(results[0].copy_text.contains("\"a\": 1"));
    }

    #[test]
    fn url_encode_decode_round_trip() {
        assert_eq!(url_encode("hello world"), "hello+world");
        assert_eq!(url_decode("hello+world").as_deref(), Some("hello world"));
    }

    #[test]
    fn converts_timestamp_to_date() {
        let results = execute_dev_tool_query("1718659200 to date");
        assert_eq!(results.len(), 1);
        assert!(results[0].copy_text.contains("2024"));
    }

    #[test]
    fn filters_catalog_by_search_text() {
        let results = search_dev_tools("sha256");
        assert!(results.iter().any(|result| result.title.contains("SHA256")));
    }

    #[test]
    fn extracts_jsonpath_value() {
        let json = r#"{"store":{"book":[{"title":"Sayings of the Century"}]}}"#;
        let value = extract_jsonpath(json, "$.store.book[0].title").unwrap();
        assert_eq!(value, "Sayings of the Century");
    }

    #[test]
    fn cron_next_runs_returns_five_lines() {
        let runs = cron_next_runs("*/5 * * * *", 5).unwrap();
        assert_eq!(runs.lines().count(), 5);
    }

    #[test]
    fn generates_qr_png_file() {
        let path = generate_qr_png("https://example.com").expect("qr path");
        assert!(path.exists());
        let _ = std::fs::remove_file(path);
    }
}