//! Keyboard-shortcut actions on windows and workspaces. Each runs on a key
//! press, so they talk to Hyprland's socket directly instead of starting
//! `hyprctl` (or Python) several times.
//!
//!   nameplate window minimize        SUPER + M: to the dock, on floating workspaces
//!   nameplate window pop             SUPER + O: float, pin on top, centre (again to undo)
//!   nameplate window width save      SUPER + ALT + HOME
//!   nameplate window width restore   SUPER + HOME
//!   nameplate window close-all       CTRL + ALT + DELETE
//!   nameplate layout [back]          SUPER + L: dwindle -> scrolling -> floating
//!   nameplate layout current [WS]
//!   nameplate layout set MODE
//!   nameplate keyboard cycle         the next keyboard layout, on every keyboard
//!   nameplate browser [--private]

use crate::json::Value;
use crate::sys;
use std::path::PathBuf;

const MODES: [&str; 3] = ["dwindle", "scrolling", "floating"];

fn active_window() -> Option<Value> {
    sys::hypr_json("activewindow").filter(|w| !w.str_of("address").is_empty())
}

fn target(window: &Value) -> String {
    format!(r#"window = "address:{}""#, window.str_of("address"))
}

fn int(v: Option<&Value>) -> Option<i64> {
    v.and_then(Value::as_f64)
        .filter(|n| n.fract() == 0.0)
        .map(|n| n as i64)
}

fn flag(window: &Value, key: &str) -> bool {
    window.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn notify_replacing(app: &str, ms: &str, text: &str, id_file: &std::path::Path) {
    let last = sys::read(id_file).unwrap_or_default();
    let mut args = vec!["-a", app, "-t", ms, "-p"];
    if last.chars().all(|c| c.is_ascii_digit()) && !last.is_empty() {
        args.extend(["-r", last.as_str()]);
    }
    args.push(text);
    if let Some(id) = sys::output("notify-send", &args) {
        let id = id.trim();
        if !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) {
            let _ = sys::write_atomic(id_file, &format!("{id}\n"));
        }
    }
}

// ------------------------------------------------------------------ windows --

/// SUPER + M. The window waits on special:minimized, tagged with the workspace
/// it came from, and its dock icon brings it back. Only floating workspaces
/// have the dock, so tiled ones don't minimize.
pub fn minimize() {
    let Some(w) = active_window() else { return };
    let Some(ws) = int(w.get("workspace").and_then(|x| x.get("id"))).filter(|n| *n >= 1) else {
        return;
    };
    if layout_of(Some(ws)) != "floating" {
        return;
    }
    let t = target(&w);
    let mine = format!("nameplate-minimized-from-{ws}");
    for tag in w
        .get("tags")
        .map(Value::as_array)
        .unwrap_or(&[])
        .iter()
        .filter_map(Value::as_str)
    {
        if tag.starts_with("nameplate-minimized-from-") && tag != mine {
            sys::hypr_dispatch(&format!(r#"hl.dsp.window.tag({{ tag = "-{tag}", {t} }})"#));
        }
    }
    sys::hypr_dispatch(&format!(r#"hl.dsp.window.tag({{ tag = "+{mine}", {t} }})"#));
    sys::hypr_dispatch(&format!(
        r#"hl.dsp.window.move({{ workspace = "special:minimized", follow = false, {t} }})"#
    ));
    sys::hypr_dispatch(&format!(r#"hl.dsp.focus({{ workspace = "{ws}" }})"#));
}

/// SUPER + O: pop the window out (float, 1300x900, centred, pinned above the
/// rest, tagged "pop" for its rounded corners), or put a popped one back.
pub fn pop() {
    let Some(w) = active_window() else { return };
    let t = target(&w);
    let d = |lua: String| sys::hypr_dispatch(&lua);
    if flag(&w, "pinned") {
        d(format!("hl.dsp.window.pin({{ {t} }})"));
        if flag(&w, "floating") {
            d(format!(r#"hl.dsp.window.float({{ action = "off", {t} }})"#));
        }
        d(format!(r#"hl.dsp.window.tag({{ tag = "-pop", {t} }})"#));
        return;
    }
    if !flag(&w, "floating") {
        d(format!(r#"hl.dsp.window.float({{ action = "on", {t} }})"#));
    }
    d(format!(
        "hl.dsp.window.resize({{ x = 1300, y = 900, relative = false, {t} }})"
    ));
    d(format!("hl.dsp.window.center({{ {t} }})"));
    d(format!("hl.dsp.window.pin({{ {t} }})"));
    d(format!(
        r#"hl.dsp.window.alter_zorder({{ mode = "top", {t} }})"#
    ));
    d(format!(r#"hl.dsp.window.tag({{ tag = "+pop", {t} }})"#));
}

fn width_store() -> PathBuf {
    sys::runtime_dir().join("nameplate-window-width")
}

/// Remember the window's width, or give it back (like Omarchy's).
pub fn width(action: &str) -> Result<(), String> {
    let Some(w) = active_window() else {
        return Ok(());
    };
    let size = w.get("size").map(Value::as_array).unwrap_or(&[]);
    let (width, height) = (
        int(size.first()).unwrap_or(0),
        int(size.get(1)).unwrap_or(0),
    );
    match action {
        "save" => {
            sys::write_atomic(width_store(), &format!("{width}\n")).map_err(|e| e.to_string())?;
            sys::output(
                "notify-send",
                &[
                    "-a",
                    "Window",
                    "-t",
                    "1500",
                    "Width saved",
                    &format!("{width}px"),
                ],
            );
        }
        "restore" => {
            let Some(saved) = sys::read(width_store()).and_then(|s| s.parse::<i64>().ok()) else {
                return Ok(());
            };
            sys::hypr_dispatch(&format!(
                "hl.dsp.window.resize({{ x = {saved}, y = {height}, relative = false, {} }})",
                target(&w)
            ));
        }
        other => return Err(format!("unknown width action: {other}")),
    }
    Ok(())
}

/// CTRL + ALT + DELETE: a normal close request to every window, so apps with
/// unsaved work can still ask.
pub fn close_all() {
    let clients = sys::hypr_json("clients");
    for c in clients.as_ref().map(Value::as_array).unwrap_or(&[]) {
        if c.get("mapped").and_then(Value::as_bool) != Some(false) {
            sys::hypr_dispatch(&format!(
                r#"hl.dsp.window.close({{ window = "address:{}" }})"#,
                c.str_of("address")
            ));
        }
    }
}

// --------------------------------------------------------- workspace layout --

fn layout_dir() -> PathBuf {
    sys::state_dir().join("workspace-layouts")
}

fn saved_layout(ws: i64) -> Option<String> {
    sys::read(layout_dir().join(ws.to_string())).filter(|m| MODES.contains(&m.as_str()))
}

/// A workspace's layout: the saved choice, else Hyprland's tiled layout.
pub fn layout_of(ws: Option<i64>) -> String {
    let (id, tiled) = match ws {
        None => {
            let a = sys::hypr_json("activeworkspace");
            (
                a.as_ref().and_then(|a| int(a.get("id"))),
                a.map(|a| a.str_of("tiledLayout").to_string()),
            )
        }
        Some(id) => {
            let all = sys::hypr_json("workspaces");
            let tiled = all
                .as_ref()
                .map(Value::as_array)
                .unwrap_or(&[])
                .iter()
                .find(|w| int(w.get("id")) == Some(id))
                .map(|w| w.str_of("tiledLayout").to_string());
            (Some(id), tiled)
        }
    };
    match id {
        Some(id) if id >= 1 => saved_layout(id)
            .or(tiled.filter(|t| !t.is_empty()))
            .unwrap_or_else(|| "dwindle".into()),
        _ => "dwindle".into(),
    }
}

/// Switch a workspace's layout through hyprland.lua's np_workspace_mode, keep
/// the choice, and tell the dock (it lives on floating workspaces).
fn apply_layout(ws: i64, mode: &str) -> bool {
    let expr = format!(r#"np_workspace_mode({ws}, "{mode}")"#);
    let ok = std::process::Command::new("hyprctl")
        .args(["eval", &expr])
        .output()
        .map(|o| {
            let all = format!(
                "{}{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            )
            .to_lowercase();
            o.status.success() && !all.contains("error")
        })
        .unwrap_or(false);
    if !ok {
        return false;
    }
    let path = layout_dir().join(ws.to_string());
    if mode == "dwindle" {
        let _ = std::fs::remove_file(path);
    } else {
        let _ = sys::write_atomic(path, &format!("{mode}\n"));
    }
    if let Some(pid) = sys::pidfile_owner(
        &sys::runtime_dir().join("nameplate-dock.pid"),
        &["nameplate-shell dock"],
    ) {
        crate::ffi::send_signal(pid, crate::ffi::SIGUSR2);
    }
    true
}

fn active_workspace() -> Option<i64> {
    sys::hypr_json("activeworkspace")
        .and_then(|a| int(a.get("id")))
        .filter(|n| *n >= 1)
}

pub fn layout(args: &[&str]) -> Result<(), String> {
    match args {
        [] | ["back"] => {
            let Some(ws) = active_workspace() else {
                return Ok(());
            };
            let now = layout_of(Some(ws));
            let i = MODES.iter().position(|m| *m == now);
            let next = match i {
                Some(i) if args == ["back"] => MODES[(i + MODES.len() - 1) % MODES.len()],
                Some(i) => MODES[(i + 1) % MODES.len()],
                None => "dwindle",
            };
            if apply_layout(ws, next) {
                let name = match next {
                    "dwindle" => "Tiling",
                    "scrolling" => "Scrolling",
                    _ => "Floating",
                };
                notify_replacing(
                    "Workspace",
                    "1500",
                    &format!("Workspace layout: {name}"),
                    &layout_dir().join("notification"),
                );
            } else {
                sys::output(
                    "notify-send",
                    &[
                        "-a",
                        "Workspace",
                        "-t",
                        "2500",
                        "Workspace layout needs a Hyprland reload",
                    ],
                );
            }
        }
        ["current"] => println!("{}", layout_of(None)),
        ["current", ws] => println!("{}", layout_of(ws.parse().ok())),
        ["set", mode] if MODES.contains(mode) => {
            if let Some(ws) = active_workspace() {
                apply_layout(ws, mode);
            }
        }
        _ => return Err(
            "usage: nameplate layout [back | current [WORKSPACE] | set dwindle|scrolling|floating]"
                .into(),
        ),
    }
    Ok(())
}

// ----------------------------------------------------------------- keyboard --

/// Switch to the next layout on every keyboard that shares the main one's list.
pub fn keyboard_cycle() {
    let Some(devices) = sys::hypr_json("devices") else {
        return;
    };
    let all = devices.get("keyboards").map(Value::as_array).unwrap_or(&[]);
    let Some(kb) = crate::bar::main_keyboard(all) else {
        return;
    };
    let layouts: Vec<&str> = kb
        .str_of("layout")
        .split(',')
        .filter(|s| !s.is_empty())
        .collect();
    if layouts.len() < 2 {
        return;
    }
    let index = int(kb.get("active_layout_index"))
        .filter(|i| (0..layouts.len() as i64).contains(i))
        .unwrap_or(0);
    let next = (index + 1) as usize % layouts.len();
    for other in all
        .iter()
        .filter(|k| k.str_of("layout") == kb.str_of("layout") && !k.str_of("name").is_empty())
    {
        sys::hypr(&format!("/switchxkblayout {} {next}", other.str_of("name")));
    }
    crate::watch::refresh(&["keyboard"]);
}

// ------------------------------------------------------------------ browser --

const BROWSER_FALLBACKS: [&str; 9] = [
    "google-chrome.desktop",
    "chromium_chromium.desktop",
    "chromium.desktop",
    "firefox_firefox.desktop",
    "firefox.desktop",
    "brave-browser.desktop",
    "vivaldi-stable.desktop",
    "microsoft-edge.desktop",
    "zen.desktop",
];

fn env_dirs(var: &str, default: &str) -> Vec<PathBuf> {
    std::env::var(var)
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.into())
        .split(':')
        .map(PathBuf::from)
        .collect()
}

/// Folders holding .desktop files, most specific first (XDG base directories).
fn application_dirs() -> Vec<PathBuf> {
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| sys::home().join(".local/share"));
    std::iter::once(data_home)
        .chain(env_dirs("XDG_DATA_DIRS", "/usr/local/share:/usr/share"))
        .map(|d| d.join("applications"))
        .collect()
}

fn desktop_file(id: &str) -> Option<PathBuf> {
    application_dirs()
        .into_iter()
        .map(|d| d.join(id))
        .find(|p| p.is_file())
}

/// The desktop entry registered for a MIME type in the mimeapps.list files
/// (what `xdg-mime query default` reads).
fn default_for(mime: &str) -> Option<String> {
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| sys::home().join(".config"));
    let mut lists = vec![config_home.join("mimeapps.list")];
    lists.extend(
        env_dirs("XDG_CONFIG_DIRS", "/etc/xdg")
            .into_iter()
            .map(|d| d.join("mimeapps.list")),
    );
    for dir in application_dirs() {
        lists.push(dir.join("mimeapps.list"));
        lists.push(dir.join("defaults.list"));
    }
    for list in lists {
        let Some(text) = sys::read(&list) else {
            continue;
        };
        let mut in_defaults = false;
        for line in text.lines().map(str::trim) {
            if line.starts_with('[') {
                in_defaults = line == "[Default Applications]";
            } else if in_defaults {
                if let Some(value) = line
                    .strip_prefix(mime)
                    .and_then(|r| r.trim_start().strip_prefix('='))
                {
                    if let Some(id) = value
                        .split(';')
                        .map(str::trim)
                        .find(|id| !id.is_empty() && desktop_file(id).is_some())
                    {
                        return Some(id.to_string());
                    }
                }
            }
        }
    }
    None
}

/// The program a desktop entry runs (its Exec line's first word).
fn desktop_exec(id: &str) -> Option<String> {
    let text = sys::read(desktop_file(id)?)?;
    let mut in_entry = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry {
            if let Some(exec) = line.strip_prefix("Exec=") {
                let first = exec.split_whitespace().next()?;
                return Some(first.trim_matches(|c| c == '"' || c == '\'').to_string());
            }
        }
    }
    None
}

pub fn browser(private: bool) -> Result<(), String> {
    let id = default_for("x-scheme-handler/http")
        .or_else(|| {
            BROWSER_FALLBACKS
                .iter()
                .find(|id| desktop_file(id).is_some())
                .map(|s| s.to_string())
        })
        .ok_or("no default web browser is installed")?;
    if !private {
        sys::spawn("gtk-launch", &[&id]);
        return Ok(());
    }
    let exe = desktop_exec(&id).ok_or("can't work out how to open a private window")?;
    let name = exe.rsplit('/').next().unwrap_or("").to_lowercase();
    let chromium = ["chrom", "brave", "vivaldi", "edge", "opera"]
        .iter()
        .any(|x| name.contains(x));
    sys::spawn(
        &exe,
        &[if chromium {
            "--incognito"
        } else {
            "--private-window"
        }],
    );
    Ok(())
}
