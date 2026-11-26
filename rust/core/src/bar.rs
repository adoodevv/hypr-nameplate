//! Bar items (Waybar custom modules). Each renders one line of JSON,
//! `{"text": ICON, "tooltip": ..., "class": [...]}`, or an empty line to hide.
//! How each one is kept up to date lives in watch.rs.

use crate::json::{self, Value};
use crate::sys;

/// One line for Waybar. `classes` drive the colours in style.css.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Item {
    pub text: String,
    pub tooltip: String,
    pub classes: Vec<String>,
    pub hidden: bool,
}

impl Item {
    fn new(text: &str, tooltip: &str, classes: &[&str]) -> Item {
        Item {
            text: text.to_string(),
            tooltip: tooltip.to_string(),
            classes: classes
                .iter()
                .filter(|c| !c.is_empty())
                .map(|c| c.to_string())
                .collect(),
            hidden: false,
        }
    }

    fn hidden() -> Item {
        Item {
            hidden: true,
            ..Item::default()
        }
    }

    /// Add the "open" class (the accent line under the icon) while this panel shows.
    fn open_when(mut self, kind: &str, route: &str) -> Item {
        let (k, r) = sys::open_panel();
        if k == kind && r == route {
            self.classes.push("open".into());
        }
        self
    }

    pub fn to_json(&self) -> String {
        if self.hidden {
            return String::new();
        }
        let classes: Vec<String> = self.classes.iter().map(|c| json::quote(c)).collect();
        format!(
            "{{\"text\": {}, \"tooltip\": {}, \"class\": [{}]}}",
            json::quote(&self.text),
            json::quote(&self.tooltip),
            classes.join(",")
        )
    }
}

/// The item's current state, for one of the names the bar config uses.
pub fn item(name: &str) -> Option<Item> {
    Some(match name {
        "logo" => Item::new("\u{f31b}", "", &[]), // Ubuntu's logo where Omarchy has its own
        "power" => Item::new("\u{f0425}", "", &[]).open_when("menu", "system"),
        "network" => network().open_when("wifi", ""),
        "bluetooth" => bluetooth().open_when("bluetooth", ""),
        "battery" => battery().open_when("power", ""),
        "monitor" => monitor().open_when("display", ""),
        "clock" => clock(),
        "awake" => awake(),
        "nightlight" => nightlight(),
        "dnd" => dnd(),
        "recording" => recording(),
        "mic" => mic(),
        "sound" => sound().open_when("sound", ""),
        "media" => media(),
        "keyboard" => keyboard(),
        "updates" => updates(),
        _ => return None,
    })
}

pub const NAMES: &str =
    "logo power network bluetooth battery monitor clock awake nightlight dnd recording mic sound media keyboard updates";

/// The Waybar signal each item listened on before it could watch for itself.
/// `nameplate refresh` still sends it, for a bar config that runs items once.
pub fn waybar_signal(name: &str) -> Option<i32> {
    Some(match name {
        "sound" => 9,
        "recording" => 10,
        "dnd" => 11,
        "logo" | "power" | "network" | "bluetooth" | "battery" | "monitor" | "keyboard" => 12,
        "mic" => 13,
        "awake" | "nightlight" => 14,
        "updates" => 15,
        "weather" => 16,
        _ => return None,
    })
}

// ------------------------------------------------------------------ network --

fn network() -> Item {
    const BARS: [&str; 5] = [
        "\u{f092f}",
        "\u{f091f}",
        "\u{f0922}",
        "\u{f0925}",
        "\u{f0928}",
    ];
    const NONE: &str = "\u{f092e}";
    let mut wifi = Vec::new();
    let mut cable_up = false;
    if let Ok(entries) = std::fs::read_dir("/sys/class/net") {
        for e in entries.flatten() {
            let path = e.path();
            // Virtual interfaces (lo, docker, VPN tunnels) live under /devices/virtual
            let real = std::fs::read_link(&path)
                .map(|t| !t.to_string_lossy().contains("/virtual/"))
                .unwrap_or(false);
            if !real {
                continue;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            if path.join("wireless").exists() || path.join("phy80211").exists() {
                wifi.push(name);
            } else if sys::read(path.join("type")).as_deref() == Some("1")
                && sys::read(path.join("operstate")).as_deref() == Some("up")
                && sys::read(path.join("carrier")).as_deref() == Some("1")
            {
                cable_up = true;
            }
        }
    }
    if cable_up {
        return Item::new("\u{f0200}", "", &["ethernet"]);
    }
    if wifi_blocked() {
        return Item::new(NONE, "", &["disabled"]);
    }
    for dev in &wifi {
        match sys::read(format!("/sys/class/net/{dev}/operstate")).as_deref() {
            Some("up") => {
                let signal = wifi_signal(dev).unwrap_or(0);
                let level = match signal {
                    s if s >= 80 => 4,
                    60..=79 => 3,
                    40..=59 => 2,
                    20..=39 => 1,
                    _ => 0,
                };
                return Item::new(BARS[level], "", &["wifi"]);
            }
            Some("dormant") => return Item::new("\u{f092b}", "", &["linked"]), // still connecting
            _ => {}
        }
    }
    Item::new(NONE, "", &["disconnected"])
}

/// Wi-Fi switched off (in the panel, airplane mode or a hardware switch).
fn wifi_blocked() -> bool {
    rfkill_blocked("wlan")
}

fn rfkill_blocked(kind: &str) -> bool {
    let Ok(entries) = std::fs::read_dir("/sys/class/rfkill") else {
        return false;
    };
    let mut seen = false;
    let mut blocked = true;
    for e in entries.flatten() {
        let p = e.path();
        if sys::read(p.join("type")).as_deref() == Some(kind) {
            seen = true;
            let soft = sys::read(p.join("soft")).as_deref() == Some("1");
            let hard = sys::read(p.join("hard")).as_deref() == Some("1");
            blocked &= soft || hard;
        }
    }
    seen && blocked
}

/// Signal strength 0-100 from the kernel's /proc/net/wireless (link quality out of 70).
fn wifi_signal(dev: &str) -> Option<u32> {
    let table = sys::read("/proc/net/wireless")?;
    for line in table.lines().skip(2) {
        let mut cols = line.split_whitespace();
        if cols.next()?.trim_end_matches(':') == dev {
            cols.next(); // status
            let quality: f64 = cols.next()?.trim_end_matches('.').parse().ok()?;
            return Some(((quality * 100.0 / 70.0).round() as u32).min(100));
        }
    }
    None
}

// ---------------------------------------------------------------- bluetooth --

fn bluetooth() -> Item {
    // /sys/class/bluetooth has "hci0" for the adapter and "hci0:256" for each
    // live connection, so a connected device needs no bluetoothctl at all.
    let mut adapter = None;
    let mut connected = false;
    if let Ok(entries) = std::fs::read_dir("/sys/class/bluetooth") {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.contains(':') {
                connected = true;
            } else if adapter.is_none() {
                adapter = Some(name);
            }
        }
    }
    let Some(adapter) = adapter else {
        return Item::hidden();
    }; // no Bluetooth: hide it
    if rfkill_blocked("bluetooth") || !powered(&adapter) {
        return Item::new("\u{f00b2}", "", &["off"]);
    }
    if connected {
        Item::new("\u{f00b1}", "", &["connected"])
    } else {
        Item::new("\u{f00af}", "", &["on"])
    }
}

/// Whether BlueZ has the adapter switched on. One quick D-Bus question via busctl
/// (bluetoothctl took a second or more to start each time).
fn powered(adapter: &str) -> bool {
    let path = format!("/org/bluez/{adapter}");
    sys::output(
        "busctl",
        &[
            "--system",
            "get-property",
            "org.bluez",
            &path,
            "org.bluez.Adapter1",
            "Powered",
        ],
    )
    .map(|out| out.trim() == "b true")
    .unwrap_or(false)
}

// ------------------------------------------------------------------ battery --

/// The first battery's sysfs folder, if this machine has one.
pub fn battery_path() -> Option<std::path::PathBuf> {
    let mut found: Vec<_> = std::fs::read_dir("/sys/class/power_supply")
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("BAT"))
        })
        .collect();
    found.sort();
    found.into_iter().next()
}

fn battery() -> Item {
    let bat = battery_path();
    let Some(bat) = bat else {
        // A desktop: Omarchy shows the power profile instead
        let profile = sys::output("powerprofilesctl", &["get"]).unwrap_or_default();
        let icon = match profile.trim() {
            "power-saver" => "\u{f032a}",
            "performance" => "\u{f04c5}",
            _ => "\u{f029a}",
        };
        return Item::new(icon, "", &[]);
    };
    let level: i32 = sys::read(bat.join("capacity"))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let status = sys::read(bat.join("status")).unwrap_or_default();
    battery_item(level, &status)
}

pub fn battery_item(level: i32, status: &str) -> Item {
    const LEVELS: [&str; 10] = [
        "\u{f007a}",
        "\u{f007b}",
        "\u{f007c}",
        "\u{f007d}",
        "\u{f007e}",
        "\u{f007f}",
        "\u{f0080}",
        "\u{f0081}",
        "\u{f0082}",
        "\u{f0079}",
    ];
    const CHARGING: [&str; 10] = [
        "\u{f089c}",
        "\u{f0086}",
        "\u{f0087}",
        "\u{f0088}",
        "\u{f089d}",
        "\u{f0089}",
        "\u{f089e}",
        "\u{f008a}",
        "\u{f008b}",
        "\u{f0085}",
    ];
    let step = ((level - 1) / 10).clamp(0, 9) as usize;
    let state = if level <= 10 {
        "critical"
    } else if level <= 20 {
        "warning"
    } else {
        ""
    };
    match status {
        "Charging" => Item::new(CHARGING[step], "", &["charging"]),
        "Full" | "Not charging" => Item::new("\u{f0085}", "", &["plugged"]),
        _ => Item::new(LEVELS[step], "", &[state]),
    }
}

// ------------------------------------------------------------------ monitor --

fn monitor() -> Item {
    let screens = sys::hypr("j/monitors")
        .and_then(|reply| json::parse(&reply))
        .map(|v| v.as_array().len())
        .unwrap_or(1);
    Item::new(
        if screens > 1 {
            "\u{f037a}"
        } else {
            "\u{f0379}"
        },
        "",
        &[],
    )
}

// -------------------------------------------------------------------- clock --

/// "Tuesday 12:02", like Omarchy (clock.rs reads the local time).
fn clock() -> Item {
    Item::new(&crate::clock::now_text(), "", &[]).open_when("calendar", "")
}

// --------------------------------------------------------- toggles (lit/off) --

fn toggle(on: bool, icon: &str, on_tip: &str, off_tip: &str) -> Item {
    if on {
        Item::new(icon, on_tip, &["on"])
    } else {
        Item::new(icon, off_tip, &["off"])
    }
}

fn awake() -> Item {
    toggle(
        sys::running_cmd("systemd-inhibit --what=idle --who=hypr-nameplate stay awake"),
        "\u{ef59}",
        "Staying awake: no screensaver or lock. Click to turn off",
        "Stay awake: click to keep the screensaver and lock away",
    )
}

fn nightlight() -> Item {
    toggle(
        sys::running("hyprsunset"),
        "\u{f050e}",
        "Night light is on. Click to turn it off",
        "Night light: click for warmer screen colours",
    )
}

fn dnd() -> Item {
    let on = sys::output("makoctl", &["mode"])
        .is_some_and(|m| m.lines().any(|l| l.trim() == "do-not-disturb"));
    toggle(
        on,
        "\u{f009b}",
        "Do not disturb is on. Click to turn it off",
        "Do not disturb: click to silence notifications",
    )
}

fn recording() -> Item {
    toggle(
        sys::running("wf-recorder"),
        "\u{f0ec2}",
        "Recording. Click to stop",
        "Record the screen",
    )
}

// -------------------------------------------------------------------- sound --

/// `wpctl get-volume` → (percent, muted). `None` when there's no such device.
pub fn volume(target: &str) -> Option<(u32, bool)> {
    let out = sys::output("wpctl", &["get-volume", target])?;
    let mut words = out.split_whitespace();
    words.next(); // "Volume:"
    let level: f64 = words.next()?.parse().ok()?;
    Some(((level * 100.0).round() as u32, out.contains("[MUTED]")))
}

fn mic() -> Item {
    match volume("@DEFAULT_AUDIO_SOURCE@") {
        None => Item::hidden(),
        Some((_, muted)) => toggle(
            muted,
            "\u{f036d}",
            "Microphone muted. Click to unmute",
            "Mute the microphone",
        ),
    }
}

fn sound() -> Item {
    let Some((vol, muted)) = volume("@DEFAULT_AUDIO_SINK@") else {
        return Item::new("\u{eee8}", "Volume: 0%", &[]);
    };
    let tip = format!("Volume: {vol}%");
    if muted {
        return Item::new("\u{eee8}", &tip, &["muted"]);
    }
    let sink = sys::output("wpctl", &["inspect", "@DEFAULT_AUDIO_SINK@"])
        .unwrap_or_default()
        .to_lowercase();
    let headphones = sink.lines().any(|l| {
        (l.contains("node.name")
            || l.contains("device.form-factor")
            || l.contains("device.icon-name"))
            && (l.contains("headphone") || l.contains("headset"))
    });
    let icon = if headphones {
        "\u{f02cb}"
    } else {
        match vol {
            v if v >= 67 => "\u{f028}",
            34..=66 => "\u{f027}",
            1..=33 => "\u{f026}",
            _ => "\u{eee8}",
        }
    };
    Item::new(icon, &tip, &[])
}

// -------------------------------------------------------------------- media --

/// What's playing through MPRIS (Spotify, a browser tab, VLC...): an icon beside
/// the clock, the song in the tooltip. Hidden when nothing is playing.
fn media() -> Item {
    let format = "{{status}}\u{1f}{{artist}}\u{1f}{{title}}\u{1f}{{playerName}}";
    let line = sys::output("playerctl", &["metadata", "--format", format]).unwrap_or_default();
    media_item(line.trim_end_matches('\n'))
}

pub fn media_item(line: &str) -> Item {
    let mut parts = line.split('\u{1f}');
    let status = parts.next().unwrap_or("");
    let artist = parts.next().unwrap_or("");
    let title = parts.next().unwrap_or("");
    let player = parts.next().unwrap_or("");
    if title.is_empty() || !matches!(status, "Playing" | "Paused") {
        return Item::hidden();
    }
    let song = if artist.is_empty() {
        title.to_string()
    } else {
        format!("{title} \u{b7} {artist}")
    };
    let mut app = player.split('.').next().unwrap_or("").to_string();
    if let Some(first) = app.get(..1) {
        app = first.to_uppercase() + &app[1..].to_lowercase();
    }
    let icon = if status == "Playing" {
        "\u{f075a}"
    } else {
        "\u{f03e4}"
    };
    let tip = format!(
        "{song}\n{app}: {}\nClick for controls",
        status.to_lowercase()
    );
    Item::new(icon, &tip, &[&status.to_lowercase()])
}

// ----------------------------------------------------------------- keyboard --

/// The active keyboard layout, only when there's more than one to switch between.
/// The keyboard you type on: not a power button or a hotkey device, the main
/// one first, one with a keymap preferred.
pub fn main_keyboard(all: &[Value]) -> Option<&Value> {
    const NOT_TYPED: [&str; 8] = [
        "power-button",
        "sleep-button",
        "video-bus",
        "wireless-hotkeys",
        "wmi-hotkeys",
        "intel-hid",
        "dell-wmi",
        "hp-wmi",
    ];
    let mut typed: Vec<&Value> = all
        .iter()
        .filter(|k| {
            let name = k.str_of("name").to_lowercase();
            !NOT_TYPED.iter().any(|x| name.contains(x))
        })
        .collect();
    typed.sort_by_key(|k| !k.get("main").and_then(Value::as_bool).unwrap_or(false));
    typed
        .iter()
        .copied()
        .find(|k| !k.str_of("active_keymap").is_empty())
        .or_else(|| typed.first().copied())
        .or_else(|| all.first())
}

fn keyboard() -> Item {
    let Some(devices) = sys::hypr("j/devices").and_then(|r| json::parse(&r)) else {
        return Item::hidden();
    };
    let all = devices.get("keyboards").map(Value::as_array).unwrap_or(&[]);
    let Some(kb) = main_keyboard(all) else {
        return Item::hidden();
    };
    let layouts: Vec<&str> = kb
        .str_of("layout")
        .split(',')
        .filter(|s| !s.is_empty())
        .collect();
    if layouts.len() < 2 {
        return Item::hidden();
    }
    let mut index = kb
        .get("active_layout_index")
        .and_then(Value::as_f64)
        .unwrap_or(0.0) as usize;
    if index >= layouts.len() {
        index = 0;
    }
    let tip = match kb.str_of("active_keymap") {
        "" => layouts[index],
        keymap => keymap,
    };
    Item::new(layouts[index], tip, &[])
}

// ------------------------------------------------------------------ updates --

/// Only shown while Ubuntu has updates waiting. Counts what an upgrade would
/// really install now (phased updates that apt holds back don't count).
fn updates() -> Item {
    let sim = sys::output(
        "apt-get",
        &["-s", "-o", "Debug::NoLocking=true", "full-upgrade"],
    )
    .unwrap_or_default();
    let n = sim.lines().filter(|l| l.starts_with("Inst ")).count();
    if n == 0 {
        return Item::hidden();
    }
    let s = if n > 1 { "s" } else { "" };
    Item::new(
        "\u{f021}",
        &format!("{n} update{s} waiting. Click to install"),
        &["waiting"],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_line_matches_the_old_scripts() {
        let it = Item::new("\u{f0928}", "", &["wifi", "", "open"]);
        assert_eq!(
            it.to_json(),
            "{\"text\": \"\u{f0928}\", \"tooltip\": \"\", \"class\": [\"wifi\",\"open\"]}"
        );
        assert_eq!(Item::hidden().to_json(), "");
    }

    #[test]
    fn media_line() {
        let it = media_item("Playing\u{1f}Burna Boy\u{1f}Last Last\u{1f}spotify");
        assert_eq!(it.text, "\u{f075a}");
        assert_eq!(it.classes, vec!["playing"]);
        assert!(it
            .tooltip
            .starts_with("Last Last \u{b7} Burna Boy\nSpotify: playing"));
        assert!(media_item("Stopped\u{1f}\u{1f}x\u{1f}vlc").hidden);
        assert!(media_item("").hidden);
    }

    #[test]
    fn battery_steps() {
        assert_eq!(battery_item(100, "Discharging").text, "\u{f0079}");
        assert_eq!(battery_item(55, "Discharging").text, "\u{f007f}"); // (55-1)/10 = step 5, as in bar-status
        assert_eq!(battery_item(15, "Discharging").classes, vec!["warning"]);
        assert_eq!(battery_item(5, "Discharging").classes, vec!["critical"]);
        assert_eq!(battery_item(0, "Discharging").text, "\u{f007a}");
        assert_eq!(battery_item(42, "Charging").classes, vec!["charging"]);
        assert_eq!(battery_item(100, "Full").classes, vec!["plugged"]);
    }
}
