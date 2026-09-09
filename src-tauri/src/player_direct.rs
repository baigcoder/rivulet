//! Direct HTTP vs torrent-engine mpv flags.
//!
//! Do not fetch a Direct URL before mpv opens it. Debrid resolvers mint a
//! one-shot link: a Range GET here spends it, and mpv then plays nothing.
//!
//! Remote HTTP is opened through the loopback proxy (`:3031`) instead, so
//! lavf talks to localhost while reqwest follows the 302 chain. That GET is
//! mpv's — not a second download.

/// Torrent engine HTTP stream — growing file, never treated as Direct HTTP.
pub fn is_engine_stream(url: &str) -> bool {
    url.starts_with("http://127.0.0.1:3030")
}

/// A second `player_start` of the same engine URL only kills the FileStream
/// that was pulling the start of the file — Buffering 0% with peers alive.
pub fn reuse_engine_process(current: Option<&str>, next: &str, running: bool) -> bool {
    running && is_engine_stream(next) && current == Some(next)
}

/// A finished copy played off disk (`file://` or a native path). Same 10-bit
/// X11 `--wid` problem as the engine HTTP path, but the file is seekable so
/// it must *not* inherit `http_seekable=0`.
pub fn is_local_file(url: &str) -> bool {
    if url.starts_with("file:") || url.starts_with('/') {
        return true;
    }
    let b = url.as_bytes();
    b.len() >= 3 && b[1] == b':' && b[0].is_ascii_alphabetic()
}

fn is_loopback(url: &str) -> bool {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"));
    let Some(rest) = rest else {
        return false;
    };
    rest.starts_with("127.0.0.1:") || rest.starts_with("localhost:") || rest.starts_with("[::1]:")
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
            // Torrent pieces arrive in bursts; a 100ms pause-wait causes
            // rapid pause/resume flicker on piece boundaries. 1s absorbs
            // the jitter without a visible stall. This is for *underruns*
            // after the picture is up — the first frame is not held for it
            // (`cache-pause-initial=no` in the backends).
            "--cache-pause=yes",
            "--cache-pause-wait=1",
            // A growing torrent is not seekable at the stream level (see
            // `stream_lavf_o`), so the only part of the film you can drag to is
            // what mpv is holding. Thirty seconds of it made the seek bar look
            // broken. These are seconds-caps on a byte budget that has not
            // changed — `demuxer-max-bytes` below is still the real ceiling, so
            // this buys roughly ten minutes of 1080p rather than any more
            // memory, and reading further ahead also keeps the swarm working
            // ahead of the playhead instead of stopping half a minute in front
            // of it.
            "--cache-secs=600",
            "--demuxer-readahead-secs=300",
            // Identify the container and start. 0.5s of analysis was another
            // GOP of HEVC sitting on "Buffering…" after the first piece.
            "--demuxer-lavf-analyzeduration=0.1",
            // Sequential from byte 0. 1 MB identifies HEVC MKV without a
            // SeekHead jump; 128 KB often is not enough and lavf then waits
            // on a range the swarm has not downloaded.
            "--demuxer-lavf-probesize=1048576",
            // Give the demuxer a generous buffer so torrent piece gaps
            // don't starve the decoder while the engine catches up.
            "--demuxer-max-bytes=150M",
            "--demuxer-max-back-bytes=50M",
            // Belt and braces only: `no` is mpv's own default, and the option
            // exists to *force* seeking on rather than to switch it off. What
            // actually decides it is `seekable`/`http_seekable` in
            // `stream_lavf_o` — do not read this line as the control.
            "--force-seekable=no",
            "--demuxer-mkv-probe-start-time=no",
            // First piece can take a while on a cold swarm.
            "--network-timeout=90",
        ]
    } else {
        &[
            "--cache-pause=no",
            "--cache-pause-initial=no",
            "--cache-pause-wait=0.05",
            "--cache-secs=20",
            "--demuxer-readahead-secs=10",
            "--demuxer-lavf-analyzeduration=0.1",
            "--demuxer-lavf-probesize=1048576",
            // First byte can wait on a debrid unlock; ffmpeg's 30s default
            // aborts that and the reconnect is the extra 10s people see.
            "--network-timeout=90",
        ]
    }
}

/// A finished torrent copy opened as `file://`. Must not inherit Direct HTTP
/// reconnect/timeouts — lavf then treats the path like a 15s network open and
/// the clock stays 0:00. Cues stay on so duration and seeking work.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn file_cli() -> &'static [&'static str] {
    &[
        "--cache-pause=no",
        "--cache-pause-initial=no",
        "--demuxer-lavf-analyzeduration=2",
        "--demuxer-lavf-probesize=1048576",
        "--demuxer-max-bytes=150M",
    ]
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn file_kv() -> &'static [(&'static str, &'static str)] {
    &[
        ("cache-pause", "no"),
        ("cache-pause-initial", "no"),
        ("demuxer-lavf-analyzeduration", "2"),
        ("demuxer-lavf-probesize", "1048576"),
        ("demuxer-max-bytes", "150M"),
    ]
}

/// Live TV (MPEG-TS / HLS). The Direct flags above are for a debrid file:
/// 512KiB and `nobuffer` start a movie in a beat. A 4K HEVC IDR is often
/// bigger than that probe, and `nobuffer` then drops the rest of the
/// picture while audio (tiny frames) keeps playing — sound, a black
/// window, a 1080p badge on a channel named 4K. Last-wins against
/// `cache_cli`, so VOD stays fast.
///
/// `cache-pause-initial` stays `no` (the backends set it), so cache-secs
/// fills *after* the first frame. Shrinking it to 8s was the live hitch:
/// every hop through `:3031` (Premium 302 → proxy → provider) is extra
/// latency Direct never sees, and Direct's 50ms pause-wait then flicker-
/// paused on every underrun. Probe stays 8MB so a 4K IDR still fits;
/// analyzeduration is the Connecting wait, not the picture.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn live_cli() -> &'static [&'static str] {
    &[
        "--cache-pause=yes",
        "--cache-pause-wait=1",
        "--cache-secs=20",
        "--demuxer-readahead-secs=10",
        "--demuxer-lavf-analyzeduration=0.5",
        "--demuxer-lavf-probesize=8388608",
        "--demuxer-lavf-o=fflags=+genpts",
        "--vd-lavc-dr=no",
        "--force-seekable=no",
    ]
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn live_kv() -> &'static [(&'static str, &'static str)] {
    &[
        ("cache-pause", "yes"),
        ("cache-pause-wait", "1"),
        ("cache-secs", "20"),
        ("demuxer-readahead-secs", "10"),
        ("demuxer-lavf-analyzeduration", "0.5"),
        ("demuxer-lavf-probesize", "8388608"),
        ("demuxer-lavf-o", "fflags=+genpts"),
        ("vd-lavc-dr", "no"),
        ("force-seekable", "no"),
    ]
}

/// `seekable` is only ever true for an engine stream whose *tail* is already
/// downloaded — see the frontend's `tailBuffered`. It is not a preference: the
/// first thing mpv does with a seekable matroska is jump to the last few
/// hundred kilobytes to parse the Cues, and on a torrent that has not got them
/// yet that both stalls the open and drags librqbit's priority window to the
/// wrong end of the file. With the tail on disk that read is free, and the
/// whole seek bar works instead of only the part mpv is holding.
pub fn stream_lavf_o(engine: bool, live: bool, seekable: bool) -> &'static str {
    if engine {
        // reconnect_on_http_error=500 makes mpv retry on the engine's
        // transient HTTP 500 (torrent still initialising) instead of
        // exiting. Only one code: mpv's --stream-lavf-o splits on commas,
        // so "500,503" would parse "503" as a keyless fragment and abort.
        // timeout 120s: the first piece on a cold swarm is slower than
        // ffmpeg's 30s default, and that abort is a black 0:00 player.
        if seekable {
            return "reconnect=1,reconnect_streamed=1,reconnect_delay_max=3,reconnect_on_http_error=500,timeout=120000000,rw_timeout=120000000";
        }
        // http_seekable is the protocol flag; seekable is what older lavf
        // still reads. Both, because one ignored is a black window at 0:00
        // while the swarm fills the wrong end.
        "reconnect=1,reconnect_streamed=1,reconnect_delay_max=3,reconnect_on_http_error=500,seekable=0,http_seekable=0,timeout=120000000,rw_timeout=120000000"
    } else if live {
        // IPTV servers are slower to answer than a debrid CDN. ffmpeg's
        // 15s default is the "connection error" on a channel that would
        // have started a moment later.
        "reconnect=1,reconnect_streamed=1,reconnect_delay_max=3,timeout=60000000,rw_timeout=60000000"
    } else {
        "reconnect=1,reconnect_streamed=1,reconnect_delay_max=1,timeout=15000000,rw_timeout=15000000"
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn cache_kv(engine: bool) -> &'static [(&'static str, &'static str)] {
    if engine {
        &[
            ("cache-pause", "yes"),
            ("cache-pause-wait", "1"),
            // See `cache_cli`: the draggable region is whatever mpv holds.
            ("cache-secs", "600"),
            ("demuxer-readahead-secs", "300"),
            ("demuxer-lavf-analyzeduration", "0.1"),
            ("demuxer-lavf-probesize", "1048576"),
            ("demuxer-max-bytes", "150M"),
            ("demuxer-max-back-bytes", "50M"),
            ("force-seekable", "no"),
            ("demuxer-mkv-probe-start-time", "no"),
            ("network-timeout", "90"),
        ]
    } else {
        &[
            ("cache-pause", "no"),
            ("cache-pause-initial", "no"),
            ("cache-pause-wait", "0.05"),
            ("cache-secs", "20"),
            ("demuxer-readahead-secs", "10"),
            ("demuxer-lavf-analyzeduration", "0.1"),
            ("demuxer-lavf-probesize", "1048576"),
            ("network-timeout", "90"),
        ]
    }
}
