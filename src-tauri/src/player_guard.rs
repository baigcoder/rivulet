//! What the page may ask mpv to do.
//!
//! `player_ipc` used to hand mpv whatever JSON the page sent, and mpv's command
//! set includes `run` and `subprocess` — start any program — plus quieter ways
//! to the same place: `keybind` with a `run` binding, `load-script`, or a
//! `script-opts` that points the ytdl hook at another executable. One script
//! that got into the page was one program running on the machine.
//!
//! So the commands are the ones `MpvPlayer.vue` actually sends, and nothing
//! else; writable properties are the families its controls touch. Reads stay
//! open — `get_property` changes nothing. A new control that needs another
//! command or property adds it here, in one place for all three backends.

/// Commands the player sends whatever their arguments.
const COMMANDS: &[&str] = &[
    "get_property",
    "show-text",
    "sub-add",
    "frame-step",
    "frame-back-step",
];

/// The three mouse bindings the player installs. A binding's action is itself
/// an mpv command, so only these exact pairs pass.
const KEYBINDS: &[(&str, &str)] = &[
    ("MBTN_LEFT", "cycle pause"),
    ("WHEEL_UP", "add volume 5"),
    ("WHEEL_DOWN", "add volume -5"),
];

/// Writable properties, exactly or by family prefix.
const PROPERTIES: &[&str] = &[
    "pause", "time-pos", "volume", "mute", "speed", "aid", "sid", "video-scale",
    "panscan", "keepaspect", "gamut-mapping-mode", "tone-mapping",
];
const PROPERTY_FAMILIES: &[&str] = &["sub-", "audio-", "target-"];

fn property_allowed(name: &str) -> bool {
    PROPERTIES.contains(&name) || PROPERTY_FAMILIES.iter().any(|p| name.starts_with(p))
}

/// Check one `{"command": [...]}` line before it reaches mpv's socket.
pub fn check(line: &str) -> Result<(), String> {
    let refuse = || Err::<(), String>("mpv command not allowed".into());
    let value: serde_json::Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
    let Some(args) = value.get("command").and_then(|c| c.as_array()) else {
        return refuse();
    };
    let arg = |i: usize| args.get(i).and_then(|a| a.as_str());
    match arg(0) {
        Some(name) if COMMANDS.contains(&name) => Ok(()),
        Some("set_property") => match arg(1) {
            Some(prop) if property_allowed(prop) => Ok(()),
            _ => refuse(),
        },
        Some("keybind") => match (arg(1), arg(2)) {
            (Some(key), Some(action)) if args.len() == 3 && KEYBINDS.contains(&(key, action)) => Ok(()),
            _ => refuse(),
        },
        _ => refuse(),
    }
}

#[cfg(test)]
mod tests {
    use super::check;

    #[test]
    fn what_the_player_sends_passes() {
        assert!(check(r#"{"command":["set_property","pause",true]}"#).is_ok());
        assert!(check(r#"{"command":["set_property","sub-font-size",40]}"#).is_ok());
        assert!(check(r#"{"command":["get_property","duration"]}"#).is_ok());
        assert!(check(r#"{"command":["sub-add","https://x/y.srt","cached","English","en"]}"#).is_ok());
        assert!(check(r#"{"command":["keybind","WHEEL_UP","add volume 5"]}"#).is_ok());
    }

    #[test]
    fn ways_to_start_a_program_are_refused() {
        assert!(check(r#"{"command":["run","calc.exe"]}"#).is_err());
        assert!(check(r#"{"command":["subprocess",["calc.exe"]]}"#).is_err());
        assert!(check(r#"{"command":["load-script","C:/x.lua"]}"#).is_err());
        assert!(check(r#"{"command":["keybind","MBTN_LEFT","run calc.exe"]}"#).is_err());
        assert!(check(r#"{"command":["set_property","script-opts","ytdl_hook-ytdl_path=calc.exe"]}"#).is_err());
        assert!(check(r#"{"command":["set_property","stream-record","C:/x"]}"#).is_err());
        assert!(check(r#"{"command":{"name":"run","args":["calc.exe"]}}"#).is_err());
        assert!(check("not json").is_err());
    }
}
