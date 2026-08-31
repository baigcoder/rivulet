// ----------------------------------------------------------------------------
// Credential redaction for anything mpv's log says out loud.
//
// A player error is the most useful thing a bug report can carry and the most
// dangerous: `log_tail` is read by the frontend, shown in an error state and
// pasted into issues — and the URL a live stream resolves to is
// `http://host:8080/live/<username>/<password>/1234.m3u8`. The account's
// password is a *path segment* of the thing that failed to open, so the line
// that explains the failure is also the line that leaks the account.
//
// Redaction happens where the log is read rather than where it is displayed:
// there is one reader per platform and any number of eventual displays, and a
// credential that never crosses the IPC boundary cannot be leaked by the next
// caller who forgets.
// ----------------------------------------------------------------------------

use std::sync::LazyLock;

use regex::Regex;

/// The Xtream path shape: `/live/user/pass/1234.ts`, and the same with
/// `movie`/`series`. Anchored on the numeric stream id and its extension,
/// which is what makes it a credential path rather than three ordinary
/// path segments.
static PATH_CREDS: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(r"(?i)(/(?:live|movie|series)/)[^/\s]+/[^/\s]+(/\d+(?:\.[a-z0-9]+)?)")
		.expect("static regex")
});

/// The same, with no `/live/` marker in front of it — some panels serve
/// `/user/pass/1234.ts` straight off the root.
static BARE_PATH_CREDS: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(r"(?i)(://[^/\s]+/)[^/\s?&]+/[^/\s?&]+(/\d+(?:\.[a-z0-9]+)?)").expect("static regex")
});

/// `?username=…&password=…`, which is how `player_api.php` and `get.php`
/// take the same two values.
static QUERY_CREDS: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(r#"(?i)\b(username|password|pass|user|token|auth)=[^&\s"']+"#).expect("static regex")
});

/// A `user:pass@host` authority.
static USERINFO: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"://[^/@\s:]+:[^/@\s]+@").expect("static regex"));

/// Replace every credential shape above with `***`, leaving the rest of the
/// line — the host, the stream id, mpv's own message — intact, because that
/// is the part that says what went wrong.
pub fn redact(text: &str) -> String {
	let out = PATH_CREDS.replace_all(text, "${1}***/***${2}");
	let out = BARE_PATH_CREDS.replace_all(&out, "${1}***/***${2}");
	let out = QUERY_CREDS.replace_all(&out, "$1=***");
	USERINFO.replace_all(&out, "://***:***@").into_owned()
}

#[cfg(test)]
mod tests {
	use super::*;

	/// The line this function exists for: mpv naming the stream it could
	/// not open, credentials and all.
	#[test]
	fn strips_xtream_path_credentials() {
		let line = "[  0.12][e][stream] Failed to open http://panel.example.com:8080/live/joe123/s3cret/4567.m3u8.";
		let out = redact(line);
		assert!(!out.contains("joe123"), "{out}");
		assert!(!out.contains("s3cret"), "{out}");
		// What is left has to still be diagnosable.
		assert!(out.contains("panel.example.com:8080"), "{out}");
		assert!(out.contains("4567.m3u8"), "{out}");
		assert!(out.contains("Failed to open"), "{out}");
	}

	#[test]
	fn strips_credentials_without_a_live_marker() {
		let out = redact("open http://host.example.com/joe123/s3cret/89.ts failed");
		assert!(!out.contains("joe123"), "{out}");
		assert!(!out.contains("s3cret"), "{out}");
		assert!(out.contains("89.ts"), "{out}");
	}

	#[test]
	fn strips_query_credentials() {
		let out = redact(
			"http://panel.example.com/player_api.php?username=joe123&password=s3cret&action=x",
		);
		assert!(!out.contains("joe123"), "{out}");
		assert!(!out.contains("s3cret"), "{out}");
		assert!(out.contains("action=x"), "{out}");
	}

	#[test]
	fn strips_url_embedded_userinfo() {
		let out = redact("rtsp://joe123:s3cret@host.example.com/ch1");
		assert!(!out.contains("joe123"), "{out}");
		assert!(!out.contains("s3cret"), "{out}");
		assert!(out.contains("host.example.com"), "{out}");
	}

	/// Redaction must not eat the ordinary log. A tail that says nothing
	/// is as useless as one that says too much.
	#[test]
	fn leaves_an_ordinary_error_alone() {
		for line in [
			"[  0.05][e][ffmpeg/demuxer] http: HTTP error 401 Unauthorized",
			"[  0.05][e][vo/gpu] Failed to create GL context",
			"[  1.20][fatal][cplayer] Could not open codec.",
			"[  0.01][e][stream] Failed to open http://127.0.0.1:3032/premium-stream/abc.def",
		] {
			assert_eq!(redact(line), line, "{line}");
		}
	}

	#[test]
	fn redacts_every_occurrence_in_a_multi_line_tail() {
		let tail = "[e] open http://h.example.com/live/u1/p1/1.ts\n[e] open http://h.example.com/live/u1/p1/2.ts";
		let out = redact(tail);
		assert!(!out.contains("p1"), "{out}");
		assert_eq!(out.lines().count(), 2);
	}
}
