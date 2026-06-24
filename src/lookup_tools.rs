use crate::{
    command::{CommandCategory, CommandResult},
    search_text::normalize_search_text,
};
use serde::Deserialize;

const DICTIONARY_API: &str = "https://api.dictionaryapi.dev/api/v2/entries/en";
const TRANSLATE_API: &str = "https://api.mymemory.translated.net/get";
const WEATHER_API: &str = "https://wttr.in";

pub fn search_lookup(search_text: &str, scope_tag: &str) -> Vec<CommandResult> {
    let normalized_search_text = normalize_search_text(search_text);
    let normalized_tag = normalize_search_text(scope_tag);

    if normalized_search_text.is_empty() {
        return lookup_catalog(&normalized_tag);
    }

    if normalized_tag == "weather" || normalized_search_text.starts_with("weather ") {
        let city = normalized_search_text
            .strip_prefix("weather ")
            .unwrap_or(&normalized_search_text);
        return vec![fetch_weather(city)];
    }

    if normalized_tag == "translate" || normalized_search_text.starts_with("translate ") {
        if let Some((text, target_lang)) = parse_translation_query(&normalized_search_text) {
            return vec![fetch_translation(&text, "en", &target_lang)];
        }
        return vec![lookup_error(
            "Translation",
            "Use: translate hello to spanish",
        )];
    }

    if normalized_tag == "define"
        || normalized_tag == "dict"
        || !normalized_search_text.contains(' ')
    {
        return vec![fetch_dictionary(&normalized_search_text)];
    }

    let mut results = Vec::new();
    if let Some((text, target_lang)) = parse_translation_query(&normalized_search_text) {
        results.push(fetch_translation(&text, "en", &target_lang));
    } else {
        results.push(fetch_dictionary(&normalized_search_text));
    }
    results
}

pub fn search_inline(query: &str) -> Vec<CommandResult> {
    let normalized = normalize_search_text(query);

    if let Some(city) = normalized.strip_prefix("weather ") {
        if !city.is_empty() {
            return vec![fetch_weather(city)];
        }
    }

    if normalized.starts_with("define ") {
        let word = normalized.strip_prefix("define ").unwrap_or("");
        if !word.is_empty() {
            return vec![fetch_dictionary(word)];
        }
    }

    if normalized.starts_with("translate ") {
        if let Some((text, target_lang)) = parse_translation_query(&normalized) {
            return vec![fetch_translation(&text, "en", &target_lang)];
        }
    }

    if normalized.starts_with("weather ") {
        return Vec::new();
    }

    Vec::new()
}

fn lookup_catalog(scope_tag: &str) -> Vec<CommandResult> {
    match scope_tag {
        "weather" => vec![hint_result(
            "Weather lookup",
            "weather london",
            "weather ",
            84,
        )],
        "translate" => vec![hint_result(
            "Translate text",
            "translate hello to spanish",
            "translate ",
            84,
        )],
        "define" | "dict" => vec![hint_result(
            "Dictionary lookup",
            "define serendipity",
            "define ",
            84,
        )],
        _ => vec![
            hint_result("Define a word", "define serendipity", "define ", 83),
            hint_result(
                "Translate text",
                "translate hello to spanish",
                "translate ",
                82,
            ),
            hint_result("Weather forecast", "weather london", "weather ", 81),
        ],
    }
}

fn hint_result(title: &str, subtitle: &str, copy_text: &str, confidence: u8) -> CommandResult {
    CommandResult::copyable_feature(title, subtitle, copy_text, CommandCategory::Lookup, confidence)
}

pub fn parse_translation_query(normalized_query: &str) -> Option<(String, String)> {
    let rest = normalized_query.strip_prefix("translate ")?;
    let (text, lang_part) = rest.rsplit_once(" to ")?;
    let text = text.trim();
    let lang_name = lang_part.trim();
    if text.is_empty() || lang_name.is_empty() {
        return None;
    }
    let lang_code = language_name_to_code(lang_name)?;
    Some((text.to_string(), lang_code))
}

pub fn language_name_to_code(name: &str) -> Option<String> {
    let normalized = normalize_search_text(name);
    Some(match normalized.as_str() {
        "spanish" | "es" | "espanol" => "es",
        "french" | "fr" | "francais" => "fr",
        "german" | "de" | "deutsch" => "de",
        "italian" | "it" => "it",
        "portuguese" | "pt" => "pt",
        "japanese" | "ja" | "jp" => "ja",
        "korean" | "ko" => "ko",
        "chinese" | "zh" | "mandarin" => "zh",
        "russian" | "ru" => "ru",
        "dutch" | "nl" => "nl",
        "english" | "en" => "en",
        "arabic" | "ar" => "ar",
        "hindi" | "hi" => "hi",
        _ => return None,
    }
    .to_string())
}

fn fetch_dictionary(word: &str) -> CommandResult {
    let encoded_word = url_encode_path_segment(word);
    let url = format!("{DICTIONARY_API}/{encoded_word}");

    match ureq::get(&url).call() {
        Ok(response) => {
            if response.status() == 404 {
                return lookup_error("Dictionary", &format!("No definition found for \"{word}\""));
            }
            if !(200..300).contains(&response.status()) {
                return lookup_error(
                    "Dictionary",
                    &format!("Dictionary API returned status {}", response.status()),
                );
            }

            let body = response.into_string().unwrap_or_default();
            match format_dictionary_response(&body, word) {
                Some(copy_text) => CommandResult::copyable_feature(
                    format!("Define {word}"),
                    truncate_preview(&copy_text, 80),
                    copy_text,
                    CommandCategory::Lookup,
                    90,
                ),
                None => lookup_error("Dictionary", "Could not parse dictionary response"),
            }
        }
        Err(error) => lookup_error("Dictionary", &format!("Network error: {error}")),
    }
}

fn format_dictionary_response(body: &str, word: &str) -> Option<String> {
    let entries: Vec<DictionaryEntry> = serde_json::from_str(body).ok()?;
    let entry = entries.first()?;
    let mut lines = vec![word.to_string()];

    if let Some(phonetic) = entry.phonetic.as_ref().filter(|value| !value.is_empty()) {
        lines.push(format!("({phonetic})"));
    }

    for meaning in &entry.meanings {
        if let Some(part) = &meaning.part_of_speech {
            lines.push(format!("[{part}]"));
        }
        for definition in meaning.definitions.iter().take(3) {
            lines.push(format!("• {}", definition.definition));
            if let Some(example) = &definition.example {
                lines.push(format!("  e.g. \"{example}\""));
            }
        }
    }

    if lines.len() <= 1 {
        return None;
    }

    Some(lines.join("\n"))
}

fn fetch_translation(text: &str, source_lang: &str, target_lang: &str) -> CommandResult {
    let url = format!(
        "{TRANSLATE_API}?q={}&langpair={source_lang}|{target_lang}",
        url_encode_query(text)
    );

    match ureq::get(&url).call() {
        Ok(response) => {
            if !(200..300).contains(&response.status()) {
                return lookup_error(
                    "Translation",
                    &format!("Translation API returned status {}", response.status()),
                );
            }

            let body = response.into_string().unwrap_or_default();
            let parsed: TranslationResponse = match serde_json::from_str(&body) {
                Ok(value) => value,
                Err(_) => return lookup_error("Translation", "Could not parse translation response"),
            };

            let translated = parsed
                .response_data
                .translated_text
                .trim()
                .to_string();

            if translated.is_empty() {
                return lookup_error("Translation", "Empty translation result");
            }

            CommandResult::copyable_feature(
                format!("Translate to {target_lang}"),
                truncate_preview(&translated, 80),
                translated,
                CommandCategory::Lookup,
                90,
            )
        }
        Err(error) => lookup_error("Translation", &format!("Network error: {error}")),
    }
}

fn fetch_weather(city: &str) -> CommandResult {
    let encoded_city = url_encode_path_segment(city);
    let url = format!("{WEATHER_API}/{encoded_city}?format=3");

    match ureq::get(&url)
        .set("User-Agent", "curl/8.0")
        .call()
    {
        Ok(response) => {
            if !(200..300).contains(&response.status()) {
                return lookup_error(
                    "Weather",
                    &format!("Weather service returned status {}", response.status()),
                );
            }

            let body = response.into_string().unwrap_or_default();
            let summary = body.trim().to_string();
            if summary.is_empty() {
                return lookup_error("Weather", "Empty weather response");
            }

            CommandResult::copyable_feature(
                format!("Weather {city}"),
                truncate_preview(&summary, 80),
                summary,
                CommandCategory::Lookup,
                90,
            )
        }
        Err(error) => lookup_error("Weather", &format!("Network error: {error}")),
    }
}

fn lookup_error(title: &str, message: &str) -> CommandResult {
    CommandResult::informational(title, message)
}

fn url_encode_query(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            b' ' => "+".to_string(),
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

fn url_encode_path_segment(value: &str) -> String {
    value
        .split_whitespace()
        .map(url_encode_query)
        .collect::<Vec<_>>()
        .join("%20")
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

#[derive(Debug, Deserialize)]
struct DictionaryEntry {
    word: Option<String>,
    phonetic: Option<String>,
    meanings: Vec<DictionaryMeaning>,
}

#[derive(Debug, Deserialize)]
struct DictionaryMeaning {
    part_of_speech: Option<String>,
    definitions: Vec<DictionaryDefinition>,
}

#[derive(Debug, Deserialize)]
struct DictionaryDefinition {
    definition: String,
    example: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TranslationResponse {
    #[serde(rename = "responseData")]
    response_data: TranslationData,
}

#[derive(Debug, Deserialize)]
struct TranslationData {
    #[serde(rename = "translatedText")]
    translated_text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_translation_lang_from_inline_query() {
        let (text, lang) = parse_translation_query("translate hello to spanish").unwrap();
        assert_eq!(text, "hello");
        assert_eq!(lang, "es");
    }

    #[test]
    fn parses_language_names_and_codes() {
        assert_eq!(language_name_to_code("spanish").as_deref(), Some("es"));
        assert_eq!(language_name_to_code("fr").as_deref(), Some("fr"));
        assert_eq!(language_name_to_code("klingon"), None);
    }

    #[test]
    fn formats_dictionary_json() {
        let json = r#"[{"word":"test","meanings":[{"partOfSpeech":"noun","definitions":[{"definition":"a procedure"}]}]}]"#;
        let formatted = format_dictionary_response(json, "test").unwrap();
        assert!(formatted.contains("test"));
        assert!(formatted.contains("procedure"));
    }

    #[test]
    fn inline_weather_query_is_recognized() {
        let normalized = normalize_search_text("weather london");
        assert!(normalized.starts_with("weather "));
        assert_eq!(normalized.strip_prefix("weather "), Some("london"));
    }
}