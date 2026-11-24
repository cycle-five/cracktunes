//! Detects yt-dlp failures caused by a login or age gate.
//!
//! A gated video fails with a message like `Sign in to confirm your age`.
//! The fallback retries such failures through the residential proxy; every
//! other failure is passed through unchanged.
//!
//! The live datacenter string is pinned by the acceptance gate. This phrase
//! list is the tunable source of truth.

/// Lowercase substrings that mark a yt-dlp failure as a login or age gate.
const GATE_PHRASES: &[&str] = &[
    "sign in to confirm your age",
    "requires login",
    "confirm you\u{2019}re not a bot",
    "confirm you're not a bot",
    "inappropriate for some users",
    "age-restricted",
];

/// True when yt-dlp's stderr names a login or age gate.
///
/// Matching is case-insensitive substring search over [`GATE_PHRASES`].
pub fn is_gate_error(stderr: &str) -> bool {
    let lower = stderr.to_lowercase();
    GATE_PHRASES.iter().any(|p| lower.contains(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_age_and_login_gates() {
        for s in [
            "Sign in to confirm your age",
            "This video requires login",
            "Sign in to confirm you\u{2019}re not a bot",
            "This video may be inappropriate for some users",
        ] {
            assert!(is_gate_error(&format!("ERROR: [youtube] XSbZidsgMfw: {s}")));
        }
    }

    #[test]
    fn ignores_benign_failures() {
        for s in [
            "Video unavailable",
            "HTTP Error 404: Not Found",
            "Private video",
            "failed with http status code: 403 Forbidden",
        ] {
            assert!(!is_gate_error(s));
        }
    }
}
