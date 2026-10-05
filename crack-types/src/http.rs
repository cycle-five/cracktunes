use crate::Error;
use reqwest::Client;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use url::{Host, Url};

/// Parse a URL string into a URL object.
pub fn parse_url(url: &str) -> Result<Url, Error> {
    Url::parse(url).map_err(Into::into)
}

/// Hosts that serve YouTube playlist pages.
const YOUTUBE_HOSTS: &[&str] = &[
    "www.youtube.com",
    "youtube.com",
    "m.youtube.com",
    "music.youtube.com",
];

/// Whether `id` is shaped like a YouTube playlist id (`PL…`, `OLAK5uy_…`,
/// `RD…`, `WL`, …): 2-64 of `[A-Za-z0-9_-]`, not starting with `-`.
///
/// 🔒 The `-` rule is the one that matters. The id ends up as yt-dlp's
/// positional argument, and songbird puts no `--` in front of it, so an id of
/// `--batch-file=/proc/self/environ` was read by yt-dlp as that option.
#[must_use]
pub fn is_youtube_playlist_id(id: &str) -> bool {
    (2..=64).contains(&id.len())
        && !id.starts_with('-')
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The canonical `https://www.youtube.com/playlist?list=<id>` for a YouTube
/// link carrying a `list` (or `playlist`) parameter, or `None` if `link` is not
/// an http(s) YouTube link or its id fails [`is_youtube_playlist_id`].
///
/// Rebuilding the URL rather than passing the link on means nothing but a
/// validated id reaches yt-dlp or the playlist page fetch.
#[must_use]
pub fn canonical_youtube_playlist_url(link: &str) -> Option<String> {
    let url = Url::parse(link.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https") || !YOUTUBE_HOSTS.contains(&url.host_str()?) {
        return None;
    }
    let id = url
        .query_pairs()
        .find(|(key, _)| key == "list" || key == "playlist")
        .map(|(_, value)| value.into_owned())?;
    is_youtube_playlist_id(&id).then(|| format!("https://www.youtube.com/playlist?list={id}"))
}

/// A string that is not an http(s) link, so it must not reach yt-dlp as one.
#[derive(crate::ThisError, Debug, Clone, Copy, PartialEq, Eq)]
#[error("not an http(s) link")]
pub struct NotAYtdlUrl;

/// `url` as yt-dlp should be given it: parsed, http(s) only, and re-serialized,
/// so the string always starts with the scheme and never with `-`.
pub fn ytdl_url(url: &str) -> Result<String, NotAYtdlUrl> {
    let url = Url::parse(url).map_err(|_| NotAYtdlUrl)?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(NotAYtdlUrl);
    }
    Ok(url.into())
}

/// 🔒 The one way to hand yt-dlp a link: songbird's [`YoutubeDl`] for `url`,
/// once [`ytdl_url`] accepts it.
///
/// songbird passes `YoutubeDl::new`'s string to yt-dlp as a bare positional
/// argument with no `--` in front of it, so a string starting with `-` is read
/// as an option. That is how a playlist id of `--batch-file=/proc/self/environ`
/// leaked the bot's environment (v0.17.2). `clippy.toml` bans `YoutubeDl::new`
/// everywhere else. Search text goes through `YoutubeDl::new_search`, whose
/// `ytsearchN:` prefix keeps it from ever starting with `-`.
///
/// [`YoutubeDl`]: songbird::input::YoutubeDl
pub fn ytdl_for_url(
    client: Client,
    url: &str,
) -> Result<songbird::input::YoutubeDl<'static>, NotAYtdlUrl> {
    let parsed = Url::parse(url).map_err(|_| NotAYtdlUrl)?;
    let args = ytdl_args_for(&parsed);
    let url = ytdl_url(url)?;
    #[expect(
        clippy::disallowed_methods,
        reason = "the one sanctioned call: `url` passed ytdl_url, so it starts with http(s)"
    )]
    let ytdl = songbird::input::YoutubeDl::new(client, url).user_args(args);
    Ok(ytdl)
}

/// SoundCloud's format sort: MP3 first.
///
/// 🪤 SoundCloud's best audio is AAC over HLS (`hls_aac_160k`), and symphonia
/// rejects it ("adts: only 1 aac frame per adts packet is supported"). The
/// track errors as it starts and the queue empties without a word. Every
/// SoundCloud track also offers MP3 (`http_mp3_*`, `hls_mp3_*`), which plays.
///
/// songbird appends its own `-f ba[abr>0][vcodec=none]/best` after these
/// args, and a later `-f` wins, so the choice is steered with `-S` instead:
/// "best audio" then means best MP3. Not applied to YouTube, where it would
/// trade the opus stream for AAC.
pub const SOUNDCLOUD_FORMAT_SORT: &str = "acodec:mp3";

/// Extra yt-dlp arguments for `url`, ahead of songbird's own.
#[must_use]
pub fn ytdl_args_for(url: &Url) -> Vec<String> {
    let soundcloud = url.host_str().is_some_and(|host| {
        let host = host.to_ascii_lowercase();
        host == "soundcloud.com" || host.ends_with(".soundcloud.com")
    });
    if soundcloud {
        vec!["-S".to_owned(), SOUNDCLOUD_FORMAT_SORT.to_owned()]
    } else {
        Vec::new()
    }
}

/// 🔒 Whether `url` is an http(s) URL whose host is, and resolves only to,
/// public internet addresses.
///
/// Links to any site we don't special-case go to yt-dlp's generic extractor,
/// which fetches whatever it is given. Without this a `/play` link could point
/// the bot at `127.0.0.1` (sleevenote listens there), the LAN, or a cloud
/// metadata endpoint, and the page title would come back in the queue embed.
///
/// Not covered: yt-dlp follows redirects itself, and it resolves the name
/// again, so a public host that redirects inward (or a DNS answer that changes
/// between the two lookups) still gets through. This closes the direct case.
pub async fn is_public_http_url(url: &Url) -> bool {
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    match url.host() {
        Some(Host::Ipv4(ip)) => is_public_ip(IpAddr::V4(ip)),
        Some(Host::Ipv6(ip)) => is_public_ip(IpAddr::V6(ip)),
        Some(Host::Domain(domain)) => {
            let port = url.port_or_known_default().unwrap_or(443);
            match tokio::net::lookup_host((domain, port)).await {
                Ok(addrs) => {
                    let mut any = false;
                    for addr in addrs {
                        if !is_public_ip(addr.ip()) {
                            return false;
                        }
                        any = true;
                    }
                    any
                },
                Err(_) => false,
            }
        },
        None => false,
    }
}

/// Whether `ip` is a public internet address: not loopback, private, link
/// local, CGNAT, multicast, documentation, benchmarking or reserved.
/// (`IpAddr::is_global` would do this, but it is still unstable.)
#[must_use]
pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_public_ipv4(ip),
        IpAddr::V6(ip) => is_public_ipv6(ip),
    }
}

fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast()
        || a == 0 // 0.0.0.0/8, "this network"
        || (a == 100 && (b & 0xc0) == 64) // 100.64.0.0/10, CGNAT
        || (a == 192 && b == 0 && c == 0) // 192.0.0.0/24, IETF protocol assignments
        || (a == 198 && (b & 0xfe) == 18) // 198.18.0.0/15, benchmarking
        || a >= 240) // 240.0.0.0/4, reserved
}

fn is_public_ipv6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_public_ipv4(v4);
    }
    let s = ip.segments();
    // 64:ff9b::/96 (NAT64) carries an IPv4 address; judge that one instead.
    if s[0] == 0x64 && s[1] == 0xff9b && s[2..6] == [0, 0, 0, 0] {
        let [a, b] = s[6].to_be_bytes();
        let [c, d] = s[7].to_be_bytes();
        return is_public_ipv4(Ipv4Addr::new(a, b, c, d));
    }
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_multicast()
        || s[0] == 0 // ::/16, incl. the deprecated IPv4-compatible range
        || (s[0] & 0xfe00) == 0xfc00 // fc00::/7, unique local
        || (s[0] & 0xffc0) == 0xfe80 // fe80::/10, link local
        || (s[0] == 0x2001 && s[1] == 0x0db8)) // 2001:db8::/32, documentation
}

/// Gets the final URL after following all redirects.
pub async fn resolve_final_url(client: Client, url: &str) -> Result<String, Error> {
    resolve_final_url2(client, parse_url(url)?)
        .await
        .map(|x| x.to_string())
}

/// Gets the final URL after following all redirects.
pub async fn resolve_final_url2(client: Client, url: Url) -> Result<Url, Error> {
    // Make a GET request, which will follow redirects by default
    let response = client.get(url.to_string()).send().await?;

    // Extract the final URL after following all redirects
    let final_url = response.url().clone();

    Ok(final_url)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn soundcloud_links_ask_yt_dlp_for_mp3() {
        for link in [
            "https://soundcloud.com/realtimechris/t-sne-the-whole-thing-into-oblivion",
            "https://m.soundcloud.com/a/b",
            "https://on.soundcloud.com/AbCd",
            "http://SoundCloud.com/a/b",
        ] {
            let url = Url::parse(link).unwrap();
            assert_eq!(
                ytdl_args_for(&url),
                ["-S", SOUNDCLOUD_FORMAT_SORT],
                "{link}"
            );
        }
    }

    #[test]
    fn other_links_keep_yt_dlps_own_choice() {
        for link in [
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://notsoundcloud.com/a/b",
            "https://soundcloud.com.example.net/a/b",
            "https://example.com/soundcloud.com/a",
        ] {
            let url = Url::parse(link).unwrap();
            assert!(ytdl_args_for(&url).is_empty(), "{link}");
        }
    }

    #[test]
    fn ytdl_for_url_passes_the_args_on() {
        let client = Client::new();
        let sc = ytdl_for_url(client.clone(), "https://soundcloud.com/a/b").unwrap();
        assert!(format!("{sc:?}").contains(SOUNDCLOUD_FORMAT_SORT), "{sc:?}");
        let yt = ytdl_for_url(client, "https://www.youtube.com/watch?v=dQw4w9WgXcQ").unwrap();
        assert!(
            !format!("{yt:?}").contains(SOUNDCLOUD_FORMAT_SORT),
            "{yt:?}"
        );
    }

    #[test]
    fn ytdl_url_takes_http_links() {
        assert_eq!(
            ytdl_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ").as_deref(),
            Ok("https://www.youtube.com/watch?v=dQw4w9WgXcQ")
        );
        assert_eq!(
            ytdl_url("http://soundcloud.com/a/b").as_deref(),
            Ok("http://soundcloud.com/a/b")
        );
        // `Url::parse` trims surrounding whitespace; the re-serialized form
        // is what yt-dlp gets, so it starts with the scheme.
        assert_eq!(
            ytdl_url("  https://youtu.be/dQw4w9WgXcQ\n").as_deref(),
            Ok("https://youtu.be/dQw4w9WgXcQ")
        );
    }

    #[test]
    fn ytdl_url_refuses_everything_else() {
        for bad in [
            "--batch-file=/proc/self/environ",
            "-o /tmp/x",
            "",
            "dQw4w9WgXcQ",
            "ytsearch:never gonna give you up",
            "ytsearch5:x",
            "file:///proc/self/environ",
            "ftp://example.com/a.mp3",
        ] {
            assert_eq!(ytdl_url(bad), Err(NotAYtdlUrl), "{bad:?} must be refused");
        }
    }

    #[test]
    fn playlist_id_shape() {
        assert!(is_youtube_playlist_id("PLFgquLnL59alCl_2TQvOiD5Vgm1hCaGSI"));
        assert!(is_youtube_playlist_id(
            "OLAK5uy_k-0vj5bNYbT9aVyUPmuqzzOwMXA-2eI_M"
        ));
        assert!(is_youtube_playlist_id("WL"));
        assert!(!is_youtube_playlist_id("--batch-file=/proc/self/environ"));
        assert!(!is_youtube_playlist_id("-PL123"));
        assert!(!is_youtube_playlist_id("PL 123"));
        assert!(!is_youtube_playlist_id("PL/../x"));
        assert!(!is_youtube_playlist_id("x"));
        assert!(!is_youtube_playlist_id(&"P".repeat(65)));
    }

    #[test]
    fn canonical_playlist_url_rebuilds_from_the_id() {
        let want = Some("https://www.youtube.com/playlist?list=PL123abc".to_string());
        for link in [
            "https://www.youtube.com/playlist?list=PL123abc",
            "https://youtube.com/watch?v=dQw4w9WgXcQ&list=PL123abc&index=3",
            "http://m.youtube.com/playlist?playlist=PL123abc",
            "https://music.youtube.com/playlist?list=PL123abc&si=xyz",
        ] {
            assert_eq!(canonical_youtube_playlist_url(link), want, "{link}");
        }
    }

    #[test]
    fn canonical_playlist_url_refuses_injection_and_other_hosts() {
        for link in [
            // Percent-decoded by the URL parser into `--batch-file=/proc/self/environ`.
            "https://www.youtube.com/playlist?list=--batch-file%3D/proc/self/environ",
            "https://www.youtube.com/playlist?list=-o%20x",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://evil.example/playlist?list=PL123abc",
            "https://www.youtube.com.evil.example/playlist?list=PL123abc",
            "file://www.youtube.com/playlist?list=PL123abc",
            "PL123abc",
        ] {
            assert_eq!(canonical_youtube_playlist_url(link), None, "{link}");
        }
    }

    #[test]
    fn public_ip_classification() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "255.255.255.255",
            "::1",
            "::",
            "fd00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
            "::ffff:169.254.169.254",
            "64:ff9b::a9fe:a9fe",
        ] {
            assert!(!is_public_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in [
            "1.1.1.1",
            "142.250.72.14",
            "2606:4700:4700::1111",
            "::ffff:8.8.8.8",
        ] {
            assert!(is_public_ip(ip.parse().unwrap()), "{ip}");
        }
    }

    #[tokio::test]
    async fn public_http_url_refuses_local_targets_without_dns() {
        for url in [
            "http://127.0.0.1:3000/",
            "http://[::1]/",
            "http://169.254.169.254/latest/meta-data/",
            "http://10.0.0.5/stream.mp3",
            "file:///etc/passwd",
            "ftp://1.1.1.1/x",
        ] {
            assert!(
                !is_public_http_url(&Url::parse(url).unwrap()).await,
                "{url}"
            );
        }
        assert!(is_public_http_url(&Url::parse("https://1.1.1.1/x.mp3").unwrap()).await);
    }

    #[tokio::test]
    async fn public_http_url_resolves_localhost_to_loopback() {
        assert!(!is_public_http_url(&Url::parse("http://localhost:3000/").unwrap()).await);
    }

    // 🪤 IGNORED, not deleted, and not gated on `env::var("CI")`. These two
    // reach httpbin.org -- a live third party -- so their pass/fail is owned by
    // someone else's uptime. They failed the v0.9.2 `coverage` run and again on
    // the v0.9.4 `Build` run, both times while passing locally, and both times
    // costing a real investigation before the cause turned out to be "httpbin
    // was unreachable from GitHub's runners".
    //
    // `#[ignore]` and not a CI guard: a guard that skips the body and returns
    // Ok is a VACUOUS PASS -- the suite reports green for a test that never
    // ran. An ignored test is reported as ignored, which is the honest answer.
    // Same reasoning as ct#448/#449 for the live-YouTube tests.
    //
    // Run them deliberately with `cargo test -p crack-types -- --ignored`.
    // The two `test_parse_url*` tests below are pure and keep running.
    #[ignore = "hits httpbin.org; a third party's uptime is not this repo's CI signal"]
    #[tokio::test]
    async fn test_resolve_final_url() {
        let client = reqwest::Client::new();
        let url = "https://httpbin.org/redirect-to?url=https://example.com";
        let final_url = resolve_final_url(client, url).await.unwrap();
        assert_eq!(final_url.as_str(), "https://example.com/");
    }

    #[ignore = "hits httpbin.org; a third party's uptime is not this repo's CI signal"]
    #[tokio::test]
    async fn test_resolve_final_url2() {
        let client = reqwest::Client::new();
        let url = Url::parse("https://httpbin.org/redirect-to?url=https://example.com").unwrap();
        let final_url = resolve_final_url2(client, url).await.unwrap();
        assert_eq!(final_url.as_str(), "https://example.com/");
    }

    #[test]
    fn test_parse_url() {
        let url = "https://example.com/";
        let parsed_url = parse_url(url).unwrap();
        assert_eq!(parsed_url.as_str(), url);
    }

    #[test]
    fn test_parse_url_invalid() {
        let url = "https://example.com:foo";
        let parsed_url = parse_url(url);
        assert!(parsed_url.is_err());
    }
}
