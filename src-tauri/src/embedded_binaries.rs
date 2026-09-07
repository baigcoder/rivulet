//! Embed the Windows mpv/ffmpeg binaries into the binary itself, so a single
//! .exe is self-contained and does not need an `mpv/` folder beside it.
//!
//! On launch the embedded bytes are extracted to
//! `<app-local-data-dir>/mpv/` once; the player and ffmpeg lookups prefer
//! that location. If extraction fails (read-only volume, AV scanner eating
//! the write) the lookup falls through to the Tauri's bundle-resources
//! resolution, which is what an installer-built copy still uses — and to
//! the system `mpv.exe` / `ffmpeg.exe` on PATH after that.
//!
//! Linux and macOS compile this module to a no-op: the former expects a
//! system `mpv`, the latter links libmpv directly (`player_macos.rs`).

use std::path::{Path, PathBuf};

/// Read the bytes of an embedded binary, if any. Compiles to a no-op outside
/// Windows because the files are not present at build time on those platforms.
#[cfg(target_os = "windows")]
fn embedded_mpv() -> Option<&'static [u8]> {
    Some(include_bytes!("../mpv/mpv.exe"))
}

#[cfg(target_os = "windows")]
fn embedded_ffmpeg() -> Option<&'static [u8]> {
    Some(include_bytes!("../mpv/ffmpeg.exe"))
}

#[cfg(target_os = "windows")]
fn embedded_license() -> Option<&'static [u8]> {
    Some(include_bytes!("../mpv/LICENSE.txt"))
}

#[cfg(not(target_os = "windows"))]
fn embedded_mpv() -> Option<&'static [u8]> {
    None
}

#[cfg(not(target_os = "windows"))]
fn embedded_ffmpeg() -> Option<&'static [u8]> {
    None
}

#[cfg(not(target_os = "windows"))]
fn embedded_license() -> Option<&'static [u8]> {
    None
}

/// The directory the embedded binaries are extracted into. Resolved lazily
/// once the first lookup asks for it, and the result is cached forever — the
/// app-local-data-dir cannot change while the process is running.
fn extract_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    use tauri::Manager;
    app.path().app_local_data_dir().ok().map(|d| d.join("mpv"))
}

/// Write a single file into `<dir>/<name>`, creating `dir` first.
/// Idempotent: if the file already exists with the expected size, do nothing.
fn write_if_missing(dir: &Path, name: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(name);
    if path.is_file() {
        if let Ok(meta) = std::fs::metadata(&path) {
            if meta.len() as usize == bytes.len() {
                return Ok(path);
            }
        }
    }
    std::fs::write(&path, bytes)?;
    Ok(path)
}

/// Extract the embedded binaries once. Returns the directory they live in,
/// or `None` if there is nothing to extract (non-Windows, build without mpv
/// resources). Errors are logged and also swallowed: a missing extract
/// directory means the lookup just falls through to the next source.
pub fn ensure_extracted(app: &tauri::AppHandle) -> Option<PathBuf> {
    let dir = extract_dir(app)?;
    let mpv = embedded_mpv();
    let ffmpeg = embedded_ffmpeg();
    let license = embedded_license();
    if mpv.is_none() && ffmpeg.is_none() && license.is_none() {
        return None;
    }
    if let Some(bytes) = mpv {
        if let Err(e) = write_if_missing(&dir, "mpv.exe", bytes) {
            eprintln!("[rivulet] extract embedded mpv.exe: {e}");
        }
    }
    if let Some(bytes) = ffmpeg {
        if let Err(e) = write_if_missing(&dir, "ffmpeg.exe", bytes) {
            eprintln!("[rivulet] extract embedded ffmpeg.exe: {e}");
        }
    }
    if let Some(bytes) = license {
        if let Err(e) = write_if_missing(&dir, "LICENSE.txt", bytes) {
            eprintln!("[rivulet] extract embedded LICENSE.txt: {e}");
        }
    }
    Some(dir)
}

/// Path to the embedded-and-extracted `mpv.exe`, if the file is on disk.
/// Returns `None` on non-Windows or when the file could not be written.
pub fn extracted_mpv_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    let dir = extract_dir(app)?;
    let path = dir.join("mpv.exe");
    path.is_file().then_some(path)
}

/// Path to the embedded-and-extracted `ffmpeg.exe`, if the file is on disk.
/// Returns `None` on non-Windows or when the file could not be written.
pub fn extracted_ffmpeg_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    let dir = extract_dir(app)?;
    let path = dir.join("ffmpeg.exe");
    path.is_file().then_some(path)
}
