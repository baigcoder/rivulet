//! Why a local server did not start.
//!
//! Rivulet runs three servers on fixed loopback ports: the torrent engine,
//! the Free TV stream proxy, and the Premium API. When one cannot take its
//! port the spawn logged a line to stderr and nothing else happened — and a
//! packaged app has no stderr anyone reads. The page then asked a server that
//! was not there, and waited.
//!
//! That is how a real failure presented: two orphaned mpv processes, left by
//! an app that had gone away, were still holding 3031 and 3032. Every launch
//! after that could bind neither, so Free TV had no proxy and Premium TV had
//! no API, and both simply span. Nothing anywhere said "the port is taken" —
//! which is the one sentence that would have ended it in seconds rather than
//! days.
//!
//! So a bind failure is recorded here and the frontend asks for it. A
//! diagnosis the user can read beats a spinner that means nothing.

use std::sync::{Mutex, OnceLock};

static FAULTS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

fn faults() -> &'static Mutex<Vec<String>> {
    FAULTS.get_or_init(|| Mutex::new(Vec::new()))
}

/// Does this error mean the port was already taken?
///
/// Matched on text rather than `ErrorKind::AddrInUse`, because the error has
/// been through `anyhow` by the time the spawn sees it and the kind is no
/// longer reachable without downcasting through a chain we do not control.
fn is_port_taken(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    // The Windows wording, the unix wording, and the Rust kind's own name.
    t.contains("only one usage of each socket address")
        || t.contains("address already in use")
        || t.contains("addrinuse")
        || t.contains("access permissions")
}

/// Record that `service` could not start on `port`.
pub fn record_bind_failure(service: &str, port: u16, err: &anyhow::Error) {
    let text = format!("{err:#}");
    let line = if is_port_taken(&text) {
        format!(
            "{service} could not start: port {port} is already in use. \
             Another copy of Rivulet, or a player process one left behind, is \
             still holding it. Close Rivulet, end any leftover mpv.exe, then \
             start Rivulet again."
        )
    } else {
        format!("{service} could not start on port {port}: {text}")
    };
    eprintln!("[startup] {line}");
    if let Ok(mut f) = faults().lock() {
        if !f.iter().any(|existing| existing == &line) {
            f.push(line);
        }
    }
}

/// Everything that failed to start, for the banner the frontend shows.
#[tauri::command]
pub fn startup_faults() -> Vec<String> {
    faults().lock().map(|f| f.clone()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::is_port_taken;

    #[test]
    fn the_port_taken_wordings_are_recognised() {
        assert!(is_port_taken(
            "Only one usage of each socket address (protocol/network address/port) is normally permitted."
        ));
        assert!(is_port_taken("Address already in use (os error 98)"));
        assert!(is_port_taken("AddrInUse"));
        // Anything else is reported verbatim rather than blamed on a port.
        assert!(!is_port_taken("No such file or directory"));
    }
}
