use crate::command::{CommandCategory, CommandResult};

#[derive(Clone, Copy)]
struct EmojiRecord {
    emoji: &'static str,
    shortcode: &'static str,
    keywords: &'static str,
}

pub fn search_emojis(search_text: &str) -> Vec<CommandResult> {
    let normalized_search_text = normalize_emoji_query(search_text);
    if normalized_search_text.is_empty() {
        return emoji_home_results();
    }

    let mut scored_emojis = emoji_records()
        .iter()
        .filter_map(|emoji| score_emoji(emoji, &normalized_search_text).map(|score| (score, emoji)))
        .collect::<Vec<_>>();

    scored_emojis.sort_by_key(|(score, emoji)| (std::cmp::Reverse(*score), emoji.shortcode));

    scored_emojis
        .into_iter()
        .take(12)
        .map(|(_, emoji)| emoji_result(*emoji))
        .collect()
}

pub fn search_colon_trigger(query: &str) -> Vec<CommandResult> {
    query
        .trim()
        .strip_prefix(':')
        .map(search_emojis)
        .unwrap_or_default()
}

fn emoji_home_results() -> Vec<CommandResult> {
    emoji_records()
        .iter()
        .take(8)
        .map(|emoji| emoji_result(*emoji))
        .collect()
}

fn emoji_result(emoji: EmojiRecord) -> CommandResult {
    CommandResult::copyable_feature(
        format!("{}  :{}:", emoji.emoji, emoji.shortcode),
        emoji.keywords,
        emoji.emoji,
        CommandCategory::Emoji,
        88,
    )
}

fn score_emoji(emoji: &EmojiRecord, normalized_search_text: &str) -> Option<u8> {
    if emoji.shortcode == normalized_search_text {
        return Some(96);
    }

    if emoji.shortcode.starts_with(normalized_search_text) {
        return Some(88);
    }

    emoji
        .keywords
        .split_whitespace()
        .any(|keyword| keyword.starts_with(normalized_search_text))
        .then_some(76)
        .or_else(|| {
            emoji
                .keywords
                .contains(normalized_search_text)
                .then_some(68)
        })
}

fn normalize_emoji_query(query: &str) -> String {
    query
        .trim()
        .trim_start_matches(':')
        .trim_end_matches(':')
        .to_lowercase()
        .replace(['_', '-'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn emoji_records() -> &'static [EmojiRecord] {
    &[
        EmojiRecord {
            emoji: "😀",
            shortcode: "grinning",
            keywords: "happy smile face",
        },
        EmojiRecord {
            emoji: "😂",
            shortcode: "joy",
            keywords: "laugh funny tears",
        },
        EmojiRecord {
            emoji: "😊",
            shortcode: "blush",
            keywords: "happy smile warm",
        },
        EmojiRecord {
            emoji: "😍",
            shortcode: "heart_eyes",
            keywords: "love heart face",
        },
        EmojiRecord {
            emoji: "🤔",
            shortcode: "thinking",
            keywords: "think question curious",
        },
        EmojiRecord {
            emoji: "👍",
            shortcode: "thumbsup",
            keywords: "yes approve good",
        },
        EmojiRecord {
            emoji: "🙏",
            shortcode: "pray",
            keywords: "thanks please gratitude",
        },
        EmojiRecord {
            emoji: "🎉",
            shortcode: "tada",
            keywords: "celebrate party success",
        },
        EmojiRecord {
            emoji: "✅",
            shortcode: "white_check_mark",
            keywords: "done check complete",
        },
        EmojiRecord {
            emoji: "❌",
            shortcode: "x",
            keywords: "no cancel close",
        },
        EmojiRecord {
            emoji: "⚠️",
            shortcode: "warning",
            keywords: "alert caution risk",
        },
        EmojiRecord {
            emoji: "🔥",
            shortcode: "fire",
            keywords: "hot popular urgent",
        },
        EmojiRecord {
            emoji: "💡",
            shortcode: "bulb",
            keywords: "idea light insight",
        },
        EmojiRecord {
            emoji: "🚀",
            shortcode: "rocket",
            keywords: "launch ship fast",
        },
        EmojiRecord {
            emoji: "❤️",
            shortcode: "heart",
            keywords: "love red favorite",
        },
        EmojiRecord {
            emoji: "📌",
            shortcode: "pushpin",
            keywords: "pin save note",
        },
        EmojiRecord {
            emoji: "📝",
            shortcode: "memo",
            keywords: "note write text",
        },
        EmojiRecord {
            emoji: "📅",
            shortcode: "calendar",
            keywords: "date schedule meeting",
        },
        EmojiRecord {
            emoji: "🔗",
            shortcode: "link",
            keywords: "url quicklink chain",
        },
        EmojiRecord {
            emoji: "💻",
            shortcode: "computer",
            keywords: "code laptop work",
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_emoji_from_colon_trigger() {
        let results = search_colon_trigger(":rocket");

        assert_eq!(results[0].copy_text, "🚀");
    }
}
