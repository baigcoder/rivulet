//! Who may talk to the servers this app runs on loopback.
//!
//! Binding to 127.0.0.1 keeps the network out, but not the user's own browser:
//! any page they open can aim a request at `http://127.0.0.1:3030`, and a POST
//! with a plain-text body is a "simple" request the browser sends without
//! asking first. CORS only hides the answer — the engine had already deleted
//! the torrent by then. So every loopback server checks two headers before it
//! does anything:
//!
//! - `Origin`, which a browser attaches to every cross-origin request that can
//!   change state, and which a page cannot forge. Present and not ours → refuse.
//!   Absent is a native client (mpv, reqwest, the Android service) or a plain
//!   media/navigation GET, whose answer the page cannot read anyway.
//! - `Host`, which is what DNS rebinding cannot fake: a page that re-points its
//!   own name at 127.0.0.1 still sends that name, never `127.0.0.1:3030`.

/// Whether an `Origin` header value is this app's own page.
///
/// The packaged webview is `tauri://localhost` (Linux, macOS) or
/// `http(s)://tauri.localhost` (Windows, Android). The dev server is any
/// localhost port — only in a debug build, so a release does not trust
/// whatever else happens to be serving pages on the machine's loopback.
pub fn is_app_origin(origin: &str) -> bool {
    if matches!(
        origin,
        "tauri://localhost" | "https://tauri.localhost" | "http://tauri.localhost"
    ) {
        return true;
    }
    cfg!(debug_assertions) && is_loopback_origin(origin)
}

fn is_loopback_origin(origin: &str) -> bool {
    ["http://localhost", "http://127.0.0.1"].iter().any(|base| {
        origin == *base
            || origin
                .strip_prefix(base)
                .and_then(|rest| rest.strip_prefix(':'))
                .is_some_and(|port| !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()))
    })
}

/// Whether a `Host` header names this machine's loopback. A missing header is
/// HTTP/1.0 or a raw client, never a browser, so it is let through.
pub fn is_loopback_host(host: Option<&str>) -> bool {
    let Some(host) = host else { return true };
    let name = match host.rsplit_once(':') {
        // `[::1]:3030` splits at the port; a bare `[::1]` has none.
        Some((name, port)) if port.bytes().all(|b| b.is_ascii_digit()) => name,
        _ => host,
    };
    matches!(
        name.to_ascii_lowercase().as_str(),
        "127.0.0.1" | "localhost" | "[::1]"
    )
}

/// The whole rule: a loopback `Host`, and an `Origin` that is either absent or
/// ours. `"null"` (a sandboxed frame, a `file:` page) is not ours.
pub fn allowed(origin: Option<&str>, host: Option<&str>) -> bool {
    is_loopback_host(host) && origin.is_none_or(is_app_origin)
}

/// Whether a URL the stream proxy was asked to fetch points back at this
/// machine. LAN addresses stay allowed — an IPTV server on the home network is
/// an ordinary thing to watch — but loopback is where the engine, the Premium
/// API and anything else the user runs locally live, and nothing a playlist
/// names has a reason to go there.
pub fn targets_this_machine(url: &url::Url) -> bool {
    use std::net::IpAddr;
    let local_ip = |ip: IpAddr| match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_unspecified() || v4.is_link_local(),
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback() || v4.is_unspecified())
        }
    };
    match url.host() {
        Some(url::Host::Ipv4(ip)) => local_ip(IpAddr::V4(ip)),
        Some(url::Host::Ipv6(ip)) => local_ip(IpAddr::V6(ip)),
        Some(url::Host::Domain(name)) => {
            let name = name.trim_end_matches('.').to_ascii_lowercase();
            name == "localhost" || name.ends_with(".localhost")
        }
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreign_origins_are_refused() {
        assert!(!allowed(Some("https://evil.example"), Some("127.0.0.1:3030")));
        assert!(!allowed(Some("null"), Some("127.0.0.1:3030")));
        assert!(!allowed(Some("http://localhost.evil.example"), Some("127.0.0.1:3030")));
        assert!(!allowed(Some("http://127.0.0.1.evil.example:80"), Some("127.0.0.1:3030")));
    }

    #[test]
    fn the_app_and_native_clients_are_let_through() {
        assert!(allowed(Some("tauri://localhost"), Some("127.0.0.1:3030")));
        assert!(allowed(Some("http://tauri.localhost"), Some("127.0.0.1:3030")));
        assert!(allowed(None, Some("127.0.0.1:3030")));
        assert!(allowed(None, None));
    }

    #[test]
    fn rebinding_is_refused_by_host() {
        assert!(!allowed(None, Some("evil.example:3030")));
        assert!(allowed(None, Some("localhost:3030")));
        assert!(allowed(None, Some("[::1]:3030")));
    }

    #[test]
    fn proxy_targets_on_this_machine_are_spotted() {
        let local = |u: &str| targets_this_machine(&url::Url::parse(u).unwrap());
        assert!(local("http://127.0.0.1:3030/torrents"));
        assert!(local("http://127.8.0.1/"));
        assert!(local("http://localhost:3032/"));
        assert!(local("http://[::1]/"));
        assert!(local("http://0.0.0.0:3030/"));
        assert!(local("http://169.254.169.254/latest"));
        assert!(!local("http://192.168.1.20:8080/live/x.ts"));
        assert!(!local("https://cdn.example.com/a.m3u8"));
    }
}
