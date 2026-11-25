//! Closed sets of label values. A label is an enum, never a free string, so a
//! title, URL or error text cannot become a time series.

macro_rules! label_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name { $($variant),+ }

        impl $name {
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text),+ }
            }
        }
    };
}

label_enum!(Source { Youtube => "youtube", Soundcloud => "soundcloud", Spotify => "spotify", File => "file", Other => "other" });
label_enum!(ResolveOutcome { Ok => "ok", Error => "error" });
label_enum!(FailureReason {
    NoResults => "no_results", Gated => "gated", Drm => "drm",
    Unavailable => "unavailable", Timeout => "timeout", Other => "other"
});
label_enum!(EndOutcome { Finished => "finished", Skipped => "skipped", Errored => "errored" });
label_enum!(CommandKind { Slash => "slash", Prefix => "prefix" });
label_enum!(GatewayEvent { Ready => "ready", Resumed => "resumed", Reconnect => "reconnect" });
label_enum!(ProxyOutcome { Recovered => "recovered", Exhausted => "exhausted", Error => "error" });
label_enum!(SearchFallbackReason { Empty => "empty", Error => "error" });
label_enum!(SearchFallbackOutcome { Found => "found", Empty => "empty", Error => "error" });
label_enum!(RequestOutcome { Ok => "ok", Error => "error" });
label_enum!(SuggestionLabelOutcome { Remembered => "remembered", Stripped => "stripped" });

impl Source {
    /// The source a URL host belongs to. `None` (a keyword search) is YouTube.
    pub fn from_host(host: Option<&str>) -> Self {
        let Some(host) = host else {
            return Self::Youtube;
        };
        let host = host.to_ascii_lowercase();
        let is = |domain: &str| host == domain || host.ends_with(&format!(".{domain}"));
        if is("youtube.com") || host == "youtu.be" {
            Self::Youtube
        } else if is("soundcloud.com") {
            Self::Soundcloud
        } else if matches!(
            host.as_str(),
            "open.spotify.com" | "play.spotify.com" | "spotify.link"
        ) {
            Self::Spotify
        } else if host == "cdn.discordapp.com" {
            Self::File
        } else {
            Self::Other
        }
    }
}

impl FailureReason {
    /// Bucket an error message. Case-insensitive substring match, first hit wins.
    pub fn classify(error_text: &str) -> Self {
        let text = error_text.to_ascii_lowercase();
        let has = |needles: &[&str]| needles.iter().any(|n| text.contains(n));
        if has(&["no results"]) {
            Self::NoResults
        } else if has(&["sign in to confirm", "confirm your age", "login required"]) {
            Self::Gated
        } else if has(&["drm"]) {
            Self::Drm
        } else if has(&["unavailable", "private video", "removed"]) {
            Self::Unavailable
        } else if has(&["timed out", "timeout"]) {
            Self::Timeout
        } else {
            Self::Other
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lower_snake(s: &str) -> bool {
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
    }

    #[test]
    fn every_label_value_is_lower_snake_ascii() {
        let all: Vec<&str> = [
            Source::Youtube,
            Source::Soundcloud,
            Source::Spotify,
            Source::File,
            Source::Other,
        ]
        .map(Source::as_str)
        .into_iter()
        .chain([ResolveOutcome::Ok, ResolveOutcome::Error].map(ResolveOutcome::as_str))
        .chain(
            [
                FailureReason::NoResults,
                FailureReason::Gated,
                FailureReason::Drm,
                FailureReason::Unavailable,
                FailureReason::Timeout,
                FailureReason::Other,
            ]
            .map(FailureReason::as_str),
        )
        .chain(
            [
                EndOutcome::Finished,
                EndOutcome::Skipped,
                EndOutcome::Errored,
            ]
            .map(EndOutcome::as_str),
        )
        .chain([CommandKind::Slash, CommandKind::Prefix].map(CommandKind::as_str))
        .chain(
            [
                GatewayEvent::Ready,
                GatewayEvent::Resumed,
                GatewayEvent::Reconnect,
            ]
            .map(GatewayEvent::as_str),
        )
        .chain(
            [
                ProxyOutcome::Recovered,
                ProxyOutcome::Exhausted,
                ProxyOutcome::Error,
            ]
            .map(ProxyOutcome::as_str),
        )
        .chain(
            [SearchFallbackReason::Empty, SearchFallbackReason::Error]
                .map(SearchFallbackReason::as_str),
        )
        .chain(
            [
                SearchFallbackOutcome::Found,
                SearchFallbackOutcome::Empty,
                SearchFallbackOutcome::Error,
            ]
            .map(SearchFallbackOutcome::as_str),
        )
        .chain([RequestOutcome::Ok, RequestOutcome::Error].map(RequestOutcome::as_str))
        .chain(
            [
                SuggestionLabelOutcome::Remembered,
                SuggestionLabelOutcome::Stripped,
            ]
            .map(SuggestionLabelOutcome::as_str),
        )
        .collect();
        assert_eq!(all.len(), 5 + 2 + 6 + 3 + 2 + 3 + 3 + 2 + 3 + 2 + 2);
        for v in all {
            assert!(lower_snake(v), "{v:?} is not lower snake ascii");
        }
    }

    #[test]
    fn source_from_host() {
        let cases = [
            ("www.youtube.com", Source::Youtube),
            ("youtu.be", Source::Youtube),
            ("music.youtube.com", Source::Youtube),
            ("soundcloud.com", Source::Soundcloud),
            ("on.soundcloud.com", Source::Soundcloud),
            ("open.spotify.com", Source::Spotify),
            ("spotify.link", Source::Spotify),
            ("cdn.discordapp.com", Source::File),
            ("bandcamp.com", Source::Other),
        ];
        for (host, want) in cases {
            assert_eq!(Source::from_host(Some(host)), want, "{host}");
        }
        assert_eq!(Source::from_host(None), Source::Youtube);
    }

    #[test]
    fn classify_known_failures() {
        let cases = [
            (
                "no results found for 'ytsearch1:Tyler Cassidy - Chris Watts (02:53)'",
                FailureReason::NoResults,
            ),
            (
                "ERROR: [soundcloud] 2406802521: This video is DRM protected",
                FailureReason::Drm,
            ),
            ("Sign in to confirm you're not a bot", FailureReason::Gated),
            ("Video unavailable", FailureReason::Unavailable),
            ("operation timed out", FailureReason::Timeout),
        ];
        for (text, want) in cases {
            assert_eq!(FailureReason::classify(text), want, "{text}");
        }
    }

    #[test]
    fn classify_unknown_is_other() {
        assert_eq!(FailureReason::classify("xyzzy 7f3a"), FailureReason::Other);
    }
}
