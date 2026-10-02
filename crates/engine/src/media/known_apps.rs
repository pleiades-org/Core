//! Players Core recognises by their Windows app identifier or their name. Unknown players still
//! work; they rank after music apps, media players and browsers.
use std::sync::Arc;

/// Ordered by how strongly an app is a music player: music apps first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AppClass {
    Music,
    /// Video and general media players.
    Player,
    Browser,
    Other,
}

impl AppClass {
    pub fn label(self) -> &'static str {
        match self {
            Self::Music => "Music app",
            Self::Player => "Media player",
            Self::Browser => "Browser",
            Self::Other => "App",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct KnownApp {
    /// Shared by every install of the app (Microsoft Store or not), so a preference follows it.
    pub key: &'static str,
    pub name: &'static str,
    pub class: AppClass,
    /// Lowercase words of an identifier. Words of at least `PREFIX_LENGTH` bytes also match the
    /// start of a word (`spotify` matches `spotifyab`); shorter ones must match whole words so
    /// `opera` does not match `operations`. Words containing a space or hyphen match anywhere.
    words: &'static [&'static str],
}

const PREFIX_LENGTH: usize = 6;

const fn app(
    key: &'static str,
    name: &'static str,
    class: AppClass,
    words: &'static [&'static str],
) -> KnownApp {
    KnownApp {
        key,
        name,
        class,
        words,
    }
}

/// Windows identifiers vary by install: the desktop Spotify reports `Spotify.exe`, the Store
/// version `SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify`. First match wins.
const KNOWN_APPS: &[KnownApp] = &[
    app("spotify", "Spotify", AppClass::Music, &["spotify"]),
    app(
        "applemusic",
        "Apple Music",
        AppClass::Music,
        &["applemusic"],
    ),
    app(
        "mediaplayer",
        "Media Player",
        AppClass::Music,
        &["zunemusic"],
    ),
    app("itunes", "iTunes", AppClass::Music, &["itunes"]),
    app("tidal", "TIDAL", AppClass::Music, &["tidal"]),
    app("deezer", "Deezer", AppClass::Music, &["deezer"]),
    app(
        "amazonmusic",
        "Amazon Music",
        AppClass::Music,
        &["amazonmusic", "amazon music"],
    ),
    app(
        "youtubemusic",
        "YouTube Music",
        AppClass::Music,
        &["youtubemusic", "youtube-music", "youtube music"],
    ),
    app("foobar2000", "foobar2000", AppClass::Music, &["foobar2000"]),
    app("musicbee", "MusicBee", AppClass::Music, &["musicbee"]),
    app("aimp", "AIMP", AppClass::Music, &["aimp"]),
    app("winamp", "Winamp", AppClass::Music, &["winamp"]),
    app("qobuz", "Qobuz", AppClass::Music, &["qobuz"]),
    app("plexamp", "Plexamp", AppClass::Music, &["plexamp"]),
    app("cider", "Cider", AppClass::Music, &["cider"]),
    app("soundcloud", "SoundCloud", AppClass::Music, &["soundcloud"]),
    app("pandora", "Pandora", AppClass::Music, &["pandora"]),
    app("vlc", "VLC", AppClass::Player, &["vlc"]),
    app("mpc", "MPC", AppClass::Player, &["mpc-hc", "mpc-be"]),
    app("potplayer", "PotPlayer", AppClass::Player, &["potplayer"]),
    app("mpv", "mpv", AppClass::Player, &["mpv"]),
    app("filmstv", "Films & TV", AppClass::Player, &["zunevideo"]),
    app("edge", "Microsoft Edge", AppClass::Browser, &["msedge"]),
    app("chrome", "Google Chrome", AppClass::Browser, &["chrome"]),
    // Firefox reports a hash of its install folder; this one is the default folder's.
    app(
        "firefox",
        "Firefox",
        AppClass::Browser,
        &["firefox", "308046b0af4a39cb"],
    ),
    app("brave", "Brave", AppClass::Browser, &["brave"]),
    app("opera", "Opera", AppClass::Browser, &["opera"]),
    app("vivaldi", "Vivaldi", AppClass::Browser, &["vivaldi"]),
    app("helium", "Helium", AppClass::Browser, &["helium"]),
    app("arc", "Arc", AppClass::Browser, &["thebrowsercompany"]),
    app("discord", "Discord", AppClass::Other, &["discord"]),
    app("teams", "Microsoft Teams", AppClass::Other, &["msteams"]),
];

/// The known app an identifier or display name belongs to.
pub fn known_app(text: &str) -> Option<&'static KnownApp> {
    let lower = text.to_lowercase();
    let words = lower
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty());
    KNOWN_APPS.iter().find(|known| {
        lower == known.name.to_lowercase()
            || known.words.iter().any(|key| {
                if key.contains([' ', '-']) {
                    lower.contains(key)
                } else {
                    words.clone().any(|word| {
                        word == *key || (key.len() >= PREFIX_LENGTH && word.starts_with(key))
                    })
                }
            })
    })
}

/// Preferences and ignore lists store this key: a known app's own key, otherwise the
/// identifier in lowercase.
pub fn app_key(app_id: &str) -> Arc<str> {
    match known_app(app_id) {
        Some(known) => known.key.into(),
        None => app_id.to_lowercase().into(),
    }
}

pub fn classify_app(app_id: &str, name: &str) -> AppClass {
    known_app(app_id)
        .or_else(|| known_app(name))
        .map_or(AppClass::Other, |known| known.class)
}

/// A readable name for a player Windows identifies only by its AppUserModelID:
/// `Spotify.exe` → `Spotify`, `Publisher.Product_hash!App` → `Product`, and
/// `Browser.QOOR667UA6MLNAR7ZY2SGJF3MA` (Chromium browsers' per-install form) → `Browser`.
pub fn known_app_name(app_id: &str) -> String {
    if let Some(known) = known_app(app_id) {
        return known.name.to_owned();
    }
    let name = match app_id.split_once('!') {
        Some((package, _)) => {
            let family = package.split('_').next().unwrap_or(package);
            family.rsplit('.').next().unwrap_or(family)
        }
        None => {
            let file = app_id.rsplit(['\\', '/']).next().unwrap_or(app_id);
            let file = file
                .strip_suffix(".exe")
                .or_else(|| file.strip_suffix(".EXE"))
                .unwrap_or(file);
            match file.split_once('.') {
                Some((name, suffix)) if is_install_hash(suffix) => name,
                _ => file,
            }
        }
    };
    if name.trim().is_empty() {
        "Media app".to_owned()
    } else {
        name.trim().to_owned()
    }
}

/// An uppercase letters-and-digits suffix that identifies one install, not a name.
fn is_install_hash(text: &str) -> bool {
    text.len() >= 16
        && text
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_and_desktop_installs_of_an_app_share_one_key() {
        for identifier in [
            "Spotify.exe",
            "SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify",
            "Spotify",
        ] {
            assert_eq!(&*app_key(identifier), "spotify", "{identifier}");
            assert_eq!(classify_app(identifier, ""), AppClass::Music);
            assert_eq!(known_app_name(identifier), "Spotify");
        }
        assert_eq!(
            classify_app("AppleInc.AppleMusicWin_nzyj5cx40ttqa!App", ""),
            AppClass::Music
        );
        assert_eq!(
            classify_app("Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic", ""),
            AppClass::Music
        );
    }

    #[test]
    fn browsers_players_and_unknown_apps_are_told_apart() {
        for (identifier, class) in [
            ("Chrome", AppClass::Browser),
            ("MSEdge", AppClass::Browser),
            ("308046B0AF4A39CB", AppClass::Browser),
            ("VideoLAN.VLC", AppClass::Player),
            ("com.github.th-ch.youtube-music", AppClass::Music),
            ("Discord.exe", AppClass::Other),
            ("Helium.QOOR667UA6MLNAR7ZY2SGJF3MA", AppClass::Browser),
            ("Contoso.Recorder_abc!App", AppClass::Other),
        ] {
            assert_eq!(classify_app(identifier, ""), class, "{identifier}");
        }
    }

    #[test]
    fn short_words_match_whole_words_only() {
        assert!(known_app("Operations.Tool").is_none());
        assert!(known_app("Opera.Browser").is_some());
        assert!(known_app("Teamspeak").is_none());
        // Long words also match the start of a word.
        assert_eq!(known_app("PotPlayerMini64").unwrap().key, "potplayer");
    }

    #[test]
    fn catalog_names_are_recognised_as_well_as_identifiers() {
        assert_eq!(classify_app("unknown", "Apple Music"), AppClass::Music);
        assert_eq!(classify_app("unknown", "Media Player"), AppClass::Music);
        assert_eq!(known_app("Amazon Music").unwrap().key, "amazonmusic");
    }

    #[test]
    fn unknown_identifiers_get_readable_names_and_lowercase_keys() {
        assert_eq!(known_app_name("Contoso.Recorder_abc!App"), "Recorder");
        assert_eq!(known_app_name(r"C:\Tools\Player.EXE"), "Player");
        assert_eq!(known_app_name(""), "Media app");
        assert_eq!(known_app_name("Thorium.ABCDEFGHIJ0123456789"), "Thorium");
        assert_eq!(
            known_app_name("Helium.QOOR667UA6MLNAR7ZY2SGJF3MA"),
            "Helium"
        );
        // A dotted name that is not a hash stays whole.
        assert_eq!(known_app_name("VideoLAN.Player"), "VideoLAN.Player");
        assert_eq!(
            &*app_key("Contoso.Recorder_abc!App"),
            "contoso.recorder_abc!app"
        );
    }
}
