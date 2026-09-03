//! Direct HTTP vs torrent-engine mpv flags.
//!
//! Do not fetch a Direct URL before mpv opens it. Debrid resolvers mint a
//! one-shot link: a Range GET here spends it, and mpv then plays nothing.
//!
//! Remote HTTP is opened through the loopback proxy (`:3031`) instead, so
//! lavf talks to localhost while reqwest follows the 302 chain. That GET is
//! mpv's — not a second download.

/// Torrent engine stream — deep cache, never treated as Direct HTTP.
pub fn is_engine_stream(url: &str) -> bool {
	url.starts_with("http://127.0.0.1:3030")
}

fn is_loopback(url: &str) -> bool {
	let rest = url
		.strip_prefix("http://")
		.or_else(|| url.strip_prefix("https://"));
	let Some(rest) = rest else {
		return false;
	};
	rest.starts_with("127.0.0.1:")
		|| rest.starts_with("localhost:")
		|| rest.starts_with("[::1]:")
}

/// URL the player should open. Remote Direct links go through `:3031` so
/// ffmpeg does not sit on each 302 hop for ~30s (a 40s start is usually
/// that timeout plus one reconnect). Loopback and non-HTTP stay as-is.
pub fn play_url(url: &str, ua: Option<&str>, referer: Option<&str>) -> String {
	if is_loopback(url) || !(url.starts_with("http://") || url.starts_with("https://")) {
		return url.to_string();
	}
	crate::iptv::commands::proxy_free_stream_url(
		url.to_string(),
		ua.map(str::to_string),
		referer.map(str::to_string),
	)
}

/// Debrid hosts reject ffmpeg/mpv's default `Lavf/…` string.
pub const STREAM_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

pub fn cache_cli(engine: bool) -> &'static [&'static str] {
	if engine {
		&[
			"--cache-pause-wait=0.4",
			"--cache-secs=20",
			"--demuxer-readahead-secs=5",
			"--demuxer-lavf-analyzeduration=0.4",
			"--demuxer-lavf-probesize=524288",
		]
	} else {
		&[
			"--cache-pause=no",
			"--cache-pause-wait=0.2",
			"--cache-secs=1",
			"--demuxer-readahead-secs=1",
			"--demuxer-lavf-analyzeduration=0.4",
			"--demuxer-lavf-probesize=524288",
			// First byte can wait on a debrid unlock; ffmpeg's 30s default
			// aborts that and the reconnect is the extra 10s people see.
			"--network-timeout=90",
		]
	}
}

pub fn stream_lavf_o(engine: bool) -> &'static str {
	if engine {
		"reconnect=1,reconnect_streamed=1,reconnect_delay_max=5"
	} else {
		"reconnect=1,reconnect_streamed=1,reconnect_delay_max=2,timeout=90000000,rw_timeout=90000000"
	}
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn cache_kv(engine: bool) -> &'static [(&'static str, &'static str)] {
	if engine {
		&[
			("cache-pause-wait", "0.4"),
			("cache-secs", "20"),
			("demuxer-readahead-secs", "5"),
			("demuxer-lavf-analyzeduration", "0.4"),
			("demuxer-lavf-probesize", "524288"),
		]
	} else {
		&[
			("cache-pause", "no"),
			("cache-pause-wait", "0.2"),
			("cache-secs", "1"),
			("demuxer-readahead-secs", "1"),
			("demuxer-lavf-analyzeduration", "0.4"),
			("demuxer-lavf-probesize", "524288"),
			("network-timeout", "90"),
		]
	}
}
