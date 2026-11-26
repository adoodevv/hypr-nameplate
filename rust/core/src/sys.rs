//! System access shared by both programs: files, processes, Hyprland IPC, paths
//! and the theme. Reads files the kernel keeps current wherever possible and only
//! starts another program when there's no file to read instead.

use crate::json::{self, Value};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// A small text file's contents without trailing whitespace.
pub fn read(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|s| s.trim_end().to_string())
}

/// Write a file atomically (temporary file, then rename), creating its folder.
pub fn write_atomic(path: impl AsRef<Path>, contents: &str) -> std::io::Result<()> {
    let path = path.as_ref();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, contents)?;
    fs::rename(tmp, path)
}

/// Run a program to completion; its standard output when it succeeded.
pub fn output(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Start a program in its own session and don't wait for it.
pub fn spawn(program: impl AsRef<std::ffi::OsStr>, args: &[&str]) {
    use std::os::unix::process::CommandExt;
    let _ = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map(|mut child| {
            // Reap it in the background so it never lingers as a zombie.
            std::thread::spawn(move || child.wait());
        });
}

// -------------------------------------------------------------------- paths --

pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| "/".into())
}

/// ~/.config/nameplate, where the installer puts everything.
pub fn app_dir() -> PathBuf {
    home().join(".config").join("nameplate")
}

pub fn bin(name: &str) -> PathBuf {
    app_dir().join("bin").join(name)
}

pub fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| "/tmp".into())
}

pub fn state_dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local").join("state"))
        .join("nameplate")
}

/// Which panel bin/panel has open: (kind, route).
pub fn open_panel() -> (String, String) {
    let text = read(runtime_dir().join("nameplate-panel-open")).unwrap_or_default();
    let mut words = text.split_whitespace();
    (
        words.next().unwrap_or("").to_string(),
        words.next().unwrap_or("").to_string(),
    )
}

/// The full path of a program on PATH (`shutil.which`).
pub fn which(program: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|p| fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0))
}

// ---------------------------------------------------------------- processes --

pub struct Process {
    pub pid: u32,
    pub name: String,
    /// Arguments joined with spaces, as `pgrep -f` matches them.
    pub cmdline: String,
}

pub fn processes() -> Vec<Process> {
    let Ok(entries) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let pid = entry.file_name().to_str()?.parse::<u32>().ok()?;
            let dir = entry.path();
            let raw = fs::read(dir.join("cmdline")).ok()?;
            if raw.is_empty() {
                return None; // a zombie or a kernel thread: not really running
            }
            let cmdline = raw
                .split(|b| *b == 0)
                .filter(|part| !part.is_empty())
                .map(String::from_utf8_lossy)
                .collect::<Vec<_>>()
                .join(" ");
            Some(Process {
                pid,
                name: read(dir.join("comm")).unwrap_or_default(),
                cmdline,
            })
        })
        .collect()
}

/// `pgrep -x NAME`
pub fn running(name: &str) -> bool {
    processes().iter().any(|p| p.name == name)
}

/// `pgrep -f TEXT`, never matching this process.
pub fn running_cmd(text: &str) -> bool {
    let me = std::process::id();
    processes()
        .iter()
        .any(|p| p.pid != me && p.cmdline.contains(text))
}

/// The pid in a pidfile, if that process is alive and its command line contains
/// one of `markers` (a recycled pid belongs to someone else).
pub fn pidfile_owner(pidfile: &Path, markers: &[&str]) -> Option<u32> {
    let pid: u32 = read(pidfile)?.parse().ok()?;
    let raw = fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let cmdline = String::from_utf8_lossy(&raw).replace('\0', " ");
    markers.iter().any(|m| cmdline.contains(m)).then_some(pid)
}

/// Claim a pidfile for this process unless another live instance holds it.
/// Returns false when one is already running.
pub fn claim_pidfile(pidfile: &Path, markers: &[&str]) -> bool {
    if let Some(pid) = pidfile_owner(pidfile, markers) {
        if pid != std::process::id() {
            return false;
        }
    }
    let _ = write_atomic(pidfile, &format!("{}\n", std::process::id()));
    true
}

// ----------------------------------------------------------------- Hyprland --

fn hypr_dir() -> Option<PathBuf> {
    let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
    Some(runtime_dir().join("hypr").join(sig))
}

/// One request over Hyprland's socket, as `hyprctl` would send it.
/// `hypr("j/clients")` is `hyprctl -j clients`.
pub fn hypr(request: &str) -> Option<String> {
    let mut sock = UnixStream::connect(hypr_dir()?.join(".socket.sock")).ok()?;
    sock.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
    sock.set_write_timeout(Some(Duration::from_secs(3))).ok()?;
    sock.write_all(request.as_bytes()).ok()?;
    let mut reply = String::new();
    sock.read_to_string(&mut reply).ok()?;
    Some(reply)
}

/// A JSON query: `hypr_json("clients")`.
pub fn hypr_json(what: &str) -> Option<Value> {
    json::parse(&hypr(&format!("j/{what}"))?)
}

/// Run a Hyprland dispatcher (Lua syntax, Hyprland 0.57+):
/// `hypr_dispatch(r#"hl.dsp.focus({ window = "address:0x1" })"#)`.
pub fn hypr_dispatch(lua: &str) -> bool {
    // The leading "/" is an empty flag list, as hyprctl sends it: without it a
    // "/" inside the arguments would be taken for the end of a flag list.
    hypr(&format!("/dispatch {lua}")).is_some_and(|r| r.trim() == "ok")
}

/// Hyprland's event stream: one `event>>data` line per change.
pub fn hypr_events() -> Option<impl BufRead> {
    let sock = UnixStream::connect(hypr_dir()?.join(".socket2.sock")).ok()?;
    Some(BufReader::new(sock))
}

// -------------------------------------------------------------------- theme --

/// The current theme's colours from ~/.config/nameplate/colors.toml (`key = "#hex"`
/// lines, written by bin/theme), with Tokyo Night as the fallback.
pub fn theme_color(key: &str) -> String {
    let fallback = match key {
        "background" => "#1a1b26",
        "foreground" => "#a9b1d6",
        "accent" => "#7aa2f7",
        "muted" => "#414868",
        "selection" => "#292e42",
        _ => "#808080",
    };
    read(app_dir().join("colors.toml"))
        .and_then(|text| {
            text.lines().find_map(|line| {
                let (k, v) = line.split_once('=')?;
                (k.trim() == key).then(|| v.trim().trim_matches('"').to_string())
            })
        })
        .filter(|v| v.starts_with('#'))
        .unwrap_or_else(|| fallback.to_string())
}

/// "#rrggbb" as 0.0-1.0 channels.
pub fn rgb(hex: &str) -> (f64, f64, f64) {
    let h = hex.trim_start_matches('#');
    let ch = |i: usize| {
        h.get(i..i + 2)
            .and_then(|s| u8::from_str_radix(s, 16).ok())
            .unwrap_or(128) as f64
            / 255.0
    };
    (ch(0), ch(2), ch(4))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_itself() {
        let me = std::process::id();
        assert!(processes().iter().any(|p| p.pid == me));
    }

    #[test]
    fn hex_colours() {
        assert_eq!(rgb("#ff0080"), (1.0, 0.0, 128.0 / 255.0));
        assert_eq!(rgb("zz"), (128.0 / 255.0, 128.0 / 255.0, 128.0 / 255.0));
    }

    #[test]
    fn pidfiles() {
        let dir = std::env::temp_dir().join(format!("np-test-{}", std::process::id()));
        let file = dir.join("x.pid");
        assert!(claim_pidfile(&file, &["nameplate"]));
        // This test binary's command line holds its own path, so it "owns" the file.
        let exe = std::env::current_exe().unwrap();
        let marker = exe.file_name().unwrap().to_str().unwrap();
        assert_eq!(pidfile_owner(&file, &[marker]), Some(std::process::id()));
        assert_eq!(pidfile_owner(&file, &["no-such-program"]), None);
        let _ = fs::remove_dir_all(dir);
    }
}
