//! The folders the page may have files written into, or opened in the file
//! manager.
//!
//! `download_url`, `reveal_path` and the engine's `output_folder` all take a
//! folder from the page, and the page is the least trusted thing in the app —
//! a folder it names could be the Startup folder, with an `.exe` written in.
//! So a folder counts only if this side has seen the user choose it: the
//! defaults the app made itself, a folder picked in a dialog *this* side
//! opened, or a typed path the user confirmed in a native dialog. The list is
//! kept in the app's config folder so a choice outlives the session.
//!
//! Android is left alone: its sandbox already decides where an app may write,
//! and its drives are named by the platform, not typed.

use std::path::{Component, Path, PathBuf};
use std::sync::{OnceLock, RwLock};

struct Folders {
    /// Where the approved list is kept. Its absence means "never asked yet",
    /// which is what the one-time migration below keys on.
    file: PathBuf,
    /// The app's own folders: always allowed for both writing and revealing.
    defaults: Vec<PathBuf>,
    /// Allowed for revealing only — the documents folder the backup is saved in.
    reveal_only: Vec<PathBuf>,
    approved: RwLock<Vec<PathBuf>>,
}

static FOLDERS: OnceLock<Folders> = OnceLock::new();

/// Load the list. Called once from setup, before any command can ask.
pub fn init(app: &tauri::AppHandle, defaults: Vec<PathBuf>) {
    use tauri::Manager;
    let file = app
        .path()
        .app_config_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("approved-folders.json");
    let approved = std::fs::read_to_string(&file)
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<PathBuf>>(&s).ok())
        .unwrap_or_default();
    let reveal_only = app.path().document_dir().into_iter().collect();
    let _ = FOLDERS.set(Folders {
        file,
        defaults,
        reveal_only,
        approved: RwLock::new(approved),
    });
}

/// `C:\a\..\b` is refused rather than resolved: a path with a parent step in
/// it is either a mistake or an attempt to walk out of an allowed folder.
fn has_parent_step(path: &Path) -> bool {
    path.components().any(|c| matches!(c, Component::ParentDir))
}

/// The real location of `path`, for a folder that may not exist yet: its
/// nearest existing ancestor resolved through links, with the rest appended.
fn resolve(path: &Path) -> Option<PathBuf> {
    let mut existing = path.to_path_buf();
    let mut rest = Vec::new();
    while !existing.exists() {
        rest.push(existing.file_name()?.to_owned());
        if !existing.pop() {
            return None;
        }
    }
    let mut real = std::fs::canonicalize(&existing).ok()?;
    real.extend(rest.into_iter().rev());
    Some(real)
}

fn under_any(path: &Path, roots: &[PathBuf]) -> bool {
    let Some(real) = resolve(path) else { return false };
    roots
        .iter()
        .filter_map(|root| resolve(root))
        .any(|root| real.starts_with(&root))
}

fn check(path: &Path, reveal: bool) -> bool {
    if cfg!(target_os = "android") {
        return true;
    }
    let Some(f) = FOLDERS.get() else { return false };
    if !path.is_absolute() || has_parent_step(path) {
        return false;
    }
    let approved = f.approved.read().map(|a| a.clone()).unwrap_or_default();
    under_any(path, &f.defaults)
        || under_any(path, &approved)
        || (reveal && under_any(path, &f.reveal_only))
}

/// May files be written into `path`?
pub fn may_write(path: &Path) -> bool {
    check(path, false)
}

/// May `path` be shown in the file manager?
pub fn may_reveal(path: &Path) -> bool {
    check(path, true)
}

fn save(f: &Folders, list: &[PathBuf]) {
    if let Some(dir) = f.file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string_pretty(list) {
        let _ = std::fs::write(&f.file, json);
    }
}

/// Approve a folder this side chose itself — a drive's `Rivulet` folder.
pub fn approve_folder(path: PathBuf) {
    approve(path);
}

fn approve(path: PathBuf) {
    let Some(f) = FOLDERS.get() else { return };
    if let Ok(mut list) = f.approved.write() {
        if !list.contains(&path) {
            list.push(path);
        }
        save(f, &list);
    }
}

/// The folder a user had chosen before folders needed approving, accepted once:
/// only while the list has never been written, which is the first launch of a
/// build with this rule. Every later call is ignored — after that, a folder
/// gets in through the dialogs below or not at all.
#[tauri::command]
pub fn folders_migrate(path: Option<String>) {
    let Some(f) = FOLDERS.get() else { return };
    if f.file.exists() {
        return;
    }
    let list: Vec<PathBuf> = path
        .map(|p| PathBuf::from(p.trim()))
        .filter(|p| p.is_absolute() && !has_parent_step(p))
        .into_iter()
        .collect();
    if let Ok(mut approved) = f.approved.write() {
        approved.extend(list.iter().cloned());
        save(f, &approved);
    }
}

/// Approve a download folder the user is choosing right now. With no `path` a
/// native folder picker opens; with one, a native dialog names it and asks.
/// Either way the answer comes from a window the page cannot click for the
/// user. Returns the approved folder, or `None` if they backed out.
///
/// `title` and `question` are the page's translated strings; the path in the
/// confirmation is added here, so what is shown is what gets approved.
#[tauri::command(async)]
pub fn folders_approve(
    app: tauri::AppHandle,
    path: Option<String>,
    title: String,
    question: String,
) -> Option<String> {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
    let chosen = match path.map(|p| p.trim().to_string()).filter(|p| !p.is_empty()) {
        None => pick_folder(&app, title)?,
        Some(typed) => {
            let typed = PathBuf::from(typed);
            if !typed.is_absolute() || has_parent_step(&typed) {
                return None;
            }
            if may_write(&typed) {
                return Some(typed.to_string_lossy().into_owned());
            }
            let yes = app
                .dialog()
                .message(format!("{question}\n\n{}", typed.display()))
                .title(title)
                .kind(MessageDialogKind::Warning)
                .buttons(MessageDialogButtons::OkCancel)
                .blocking_show();
            if !yes {
                return None;
            }
            typed
        }
    };
    approve(chosen.clone());
    Some(chosen.to_string_lossy().into_owned())
}

#[cfg(desktop)]
fn pick_folder(app: &tauri::AppHandle, title: String) -> Option<PathBuf> {
    use tauri_plugin_dialog::DialogExt;
    app.dialog()
        .file()
        .set_title(title)
        .blocking_pick_folder()
        .and_then(|p| p.into_path().ok())
}

/// Android lists its drives instead of offering a chooser.
#[cfg(not(desktop))]
fn pick_folder(_app: &tauri::AppHandle, _title: String) -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parent_steps_are_spotted() {
        assert!(has_parent_step(Path::new("/a/../b")));
        assert!(!has_parent_step(Path::new("/a/b")));
    }

    #[test]
    fn a_folder_inside_a_root_is_under_it_even_before_it_exists() {
        let root = std::env::temp_dir();
        let inside = root.join("rivulet-folders-test").join("not-yet");
        assert!(under_any(&inside, std::slice::from_ref(&root)));
        let elsewhere = std::env::current_dir().unwrap();
        if !elsewhere.starts_with(&root) {
            assert!(!under_any(&elsewhere, &[root.join("rivulet-folders-test")]));
        }
    }
}
