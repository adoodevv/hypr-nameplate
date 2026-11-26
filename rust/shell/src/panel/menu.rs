//! Everything in one place (SUPER + SPACE, or the logo in the bar), laid out
//! like Omarchy's Walker menu: a narrow centred box with a plain search line
//! whose prompt names the menu you're in ("Go…", "Appearance…"), and big rows
//! underneath. Type to search everything at once, Enter to pick, arrows to move,
//! Escape or Backspace to go back a level (no back button, like Walker).
//!
//! `nameplate-shell panel menu system` opens System directly, and
//! `menu trigger/capture` opens Capture inside Trigger.

use super::{clear, hbox, label, out, row, vbox, Anchor, Panel};
use gtk::gdk;
use gtk::prelude::*;
use nameplate::json::{self, Value};
use nameplate::sys;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const MAX_RESULTS: usize = 12;

/// What picking an item does.
#[derive(Clone)]
enum Action {
    /// A submenu.
    Sub(Vec<Item>),
    /// A submenu read when it's opened (notifications, fonts...).
    Lazy(fn() -> Vec<Item>),
    /// Open another panel.
    Panel(&'static str),
    /// Run a command.
    Run(Vec<String>),
    /// Nothing: a line of information.
    Info,
}

#[derive(Clone)]
struct Item {
    icon: String,
    label: String,
    right: String,
    /// Run without closing the menu (to press again).
    stay: bool,
    action: Action,
}

fn item(icon: &str, label: &str, action: Action) -> Item {
    Item {
        icon: icon.into(),
        label: label.into(),
        right: String::new(),
        stay: false,
        action,
    }
}

impl Item {
    fn right(mut self, r: &str) -> Self {
        self.right = r.into();
        self
    }
    fn stay(mut self) -> Self {
        self.stay = true;
        self
    }
    fn info(&self) -> bool {
        matches!(self.action, Action::Info)
    }
    fn items(&self) -> Option<Vec<Item>> {
        match &self.action {
            Action::Sub(items) => Some(items.clone()),
            Action::Lazy(f) => Some(f()),
            _ => None,
        }
    }
}

fn run(cmd: &[&str]) -> Action {
    Action::Run(cmd.iter().map(|s| s.to_string()).collect())
}

fn bin(name: &str) -> String {
    sys::bin(name).to_string_lossy().into_owned()
}

/// Run a command in a floating terminal, waiting for a key before closing.
fn in_terminal(title: &str, command: &str) -> Action {
    let script = format!("{command}; echo; read -n 1 -s -r -p \"Done. Press any key to close.\"");
    run(&[
        "alacritty",
        "--class",
        "nameplate-float",
        "--title",
        title,
        "-e",
        "bash",
        "-c",
        &script,
    ])
}

/// Open a config file in the first editor found.
fn edit(path: &str) -> Action {
    for editor in ["code", "gnome-text-editor", "gedit"] {
        if sys::which(editor).is_some() {
            return run(&[editor, path]);
        }
    }
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nano".into());
    run(&["alacritty", "-e", &editor, path])
}

fn state_flag(name: &str) -> bool {
    sys::state_dir().join(name).exists()
}

fn on_off(on: bool) -> &'static str {
    if on {
        "on"
    } else {
        "off"
    }
}

// ------------------------------------------------------------------- tree --

/// Omarchy 4.5's "Go" menu, in its order.
fn tree() -> Vec<Item> {
    let b = |name: &str| bin(name);
    let recording = sys::running("wf-recorder");
    let dnd = out("makoctl", &["mode"])
        .split_whitespace()
        .any(|m| m == "do-not-disturb");
    let awake = sys::running_cmd("systemd-inhibit --what=idle --who=hypr-nameplate stay awake");
    let night = sys::running("hyprsunset");
    let cfg = sys::home().join(".config");
    let configs = [
        ("Hyprland (keys, windows, startup)", "hypr/hyprland.lua"),
        ("Lock screen", "hypr/hyprlock.conf"),
        ("Idle and screen off", "hypr/hypridle.conf"),
        ("Bar", "waybar/config.jsonc"),
        ("Bar style", "waybar/style.css"),
        ("Notifications", "mako/config"),
        ("Launcher", "fuzzel/fuzzel.ini"),
        ("Terminal", "alacritty/alacritty.toml"),
        ("Prompt", "starship.toml"),
    ];
    let has = |p: &str| sys::which(p).is_some();

    let mut updates = vec![item(
        "\u{f03d3}",
        "Ubuntu packages",
        in_terminal("Update", "sudo apt update && sudo apt full-upgrade"),
    )];
    if has("snap") {
        updates.push(item(
            "\u{f01a7}",
            "Snaps",
            in_terminal("Update", "sudo snap refresh"),
        ));
    }
    if has("flatpak") {
        updates.push(item(
            "\u{f01a7}",
            "Flatpaks",
            in_terminal("Update", "flatpak update"),
        ));
    }
    if has("fwupdmgr") {
        updates.push(item(
            "\u{f061a}",
            "Firmware",
            in_terminal("Update", "fwupdmgr refresh; fwupdmgr update"),
        ));
    }
    let shot = b("screenrecord");
    let record = if recording {
        vec![item("\u{f04db}", "Stop recording", run(&[&shot]))]
    } else {
        vec![
            item("\u{f019e}", "Region", run(&[&shot])),
            item("\u{f0293}", "Full screen", run(&[&shot, "full"])),
            item(
                "\u{f036c}",
                "Full screen + mic",
                run(&[&shot, "full", "mic"]),
            ),
        ]
    };
    if has("timeshift") {
        updates.insert(
            0,
            item(
                "\u{f0c6f}",
                "Snapshot now",
                in_terminal("Snapshot", "sudo timeshift --create --comments \"From the menu (hypr-nameplate)\" --scripted"),
            ),
        );
        updates.push(item(
            "\u{f02da}",
            "Snapshots (Timeshift)",
            run(&["timeshift-launcher"]),
        ));
    }
    let mut sources = vec![("apt", "Ubuntu packages (apt)", "\u{f03d3}")];
    if has("snap") {
        sources.push(("snap", "Snaps", "\u{f01a7}"));
    }
    if has("flatpak") {
        sources.push(("flatpak", "Flatpaks (Flathub)", "\u{f01a7}"));
    }
    let pkg = b("pkg");
    let package = |verb: &str| -> Vec<Item> {
        sources
            .iter()
            .map(|(src, text, icon)| item(icon, text, run(&[&pkg, verb, src])))
            .collect()
    };
    let toggle = b("toggle");
    let blur = lock_blur();
    let appearance = vec![
        item("\u{f0e0c}", "Theme", Action::Panel("themes")),
        item("\u{e659}", "Font", Action::Lazy(font_items)),
        item("\u{f03e}", "Background", Action::Panel("wallpapers")),
        item(
            "\u{f04ad}",
            "Next background",
            run(&[&b("nameplate-shell"), "wallpaper", "next"]),
        )
        .stay(),
        item("\u{f0e79}", "Gaps", run(&[&toggle, "gaps"])).right(on_off(!state_flag("no-gaps"))),
        item("\u{f0a27}", "Square window", run(&[&toggle, "square"]))
            .right(on_off(state_flag("square"))),
        item("\u{f0a1f}", "Dock", run(&[&toggle, "dock"])).right(on_off(!state_flag("no-dock"))),
        item("\u{f0d9d}", "Animations", run(&[&toggle, "animations"]))
            .right(on_off(!state_flag("no-animations"))),
        item(
            "\u{f033e}",
            "Lock screen blur",
            Action::Sub(
                ["off", "light", "strong"]
                    .iter()
                    .map(|level| {
                        item(
                            "\u{f033e}",
                            &super::capitalize(level),
                            run(&[&toggle, "lock-blur", level]),
                        )
                        .right(if blur == *level { "current" } else { "" })
                    })
                    .collect(),
            ),
        )
        .right(blur),
        item(
            "\u{f0209}",
            "Show/hide the bar",
            run(&["pkill", "-SIGUSR1", "waybar"]),
        ),
        item(
            "\u{f0450}",
            "Restart the bar",
            run(&["sh", "-c", "pkill -x waybar; sleep 0.5; exec waybar"]),
        ),
    ];
    let screenshot = b("screenshot");
    vec![
        item("\u{f003b}", "Apps", Action::Panel("apps")),
        item(
            "\u{f09d1}",
            "Learn",
            Action::Sub(vec![
                item("\u{f11c}", "Keybindings", run(&[&b("keys")])),
                item(
                    "\u{f359}",
                    "Hyprland wiki",
                    run(&["xdg-open", "https://wiki.hypr.land"]),
                ),
                item(
                    "\u{f02a4}",
                    "hypr-nameplate on GitHub",
                    run(&["xdg-open", "https://github.com/adoodevv/hypr-nameplate"]),
                ),
            ]),
        ),
        item("\u{ebcf}", "Appearance", Action::Sub(appearance)),
        item(
            "\u{f14de}",
            "Trigger",
            Action::Sub(vec![
                // SUPER + CTRL + C
                item(
                    "\u{f0100}",
                    "Capture",
                    Action::Sub(vec![
                        item(
                            "\u{f0100}",
                            "Screenshot",
                            Action::Sub(vec![
                                item("\u{f019e}", "Region", run(&[&screenshot, "region"])),
                                item("\u{f0293}", "Full screen", run(&[&screenshot, "full"])),
                            ]),
                        ),
                        if recording {
                            item("\u{f044a}", "Stop recording", record[0].action.clone())
                        } else {
                            item("\u{f044a}", "Screen recording", Action::Sub(record))
                        },
                        item("\u{f0266}", "Colour picker", run(&[&b("colour-picker")])),
                        item("\u{f09ed}", "Text from screen", run(&[&b("ocr")])),
                    ]),
                ),
                // SUPER + CTRL + O
                item(
                    "\u{f0a1f}",
                    "Toggle",
                    Action::Sub(vec![
                        item("\u{f009b}", "Do not disturb", run(&[&b("dnd")])).right(on_off(dnd)),
                        item("\u{f0176}", "Stay awake", run(&[&b("stay-awake")]))
                            .right(on_off(awake)),
                        item("\u{f050e}", "Night light", run(&[&b("nightlight")]))
                            .right(on_off(night)),
                        item(
                            "\u{f0209}",
                            "Top bar",
                            run(&["pkill", "-SIGUSR1", "waybar"]),
                        ),
                        item("\u{f0e79}", "Window gaps", run(&[&toggle, "gaps"]))
                            .right(on_off(!state_flag("no-gaps"))),
                        item("\u{f0a27}", "Square window", run(&[&toggle, "square"]))
                            .right(on_off(state_flag("square"))),
                        item(
                            "\u{f0322}",
                            "Laptop screen",
                            run(&[&b("display"), "laptop"]),
                        ),
                        item(
                            "\u{f1483}",
                            "Mirror screens",
                            run(&[&b("display"), "mirror"]),
                        ),
                    ]),
                ),
                item("\u{f0147}", "Clipboard", Action::Panel("clipboard")),
                item(
                    "\u{f009a}",
                    "Notifications",
                    Action::Lazy(notification_items),
                ),
                item("\u{f0e1e}", "Reminders", Action::Lazy(reminder_items)),
                item("\u{f0f8c}", "Transcode", run(&[&b("transcode")])),
                item("\u{f06a9}", "AI agents", Action::Panel("agents")),
                item("\u{f01f5}", "Emoji", Action::Panel("emoji")),
            ]),
        ),
        item(
            "\u{e615}",
            "Setup",
            Action::Sub(vec![
                item("\u{e638}", "Audio", Action::Panel("sound")),
                item("\u{f0928}", "Wi-Fi", Action::Panel("wifi")),
                item("\u{f00af}", "Bluetooth", Action::Panel("bluetooth")),
                item("\u{f0379}", "Monitors", Action::Panel("display")),
                item("\u{f140b}", "Power profile", Action::Panel("power")),
                // SUPER + CTRL + H
                item("\u{f061a}", "Hardware", Action::Lazy(hardware_items)),
                item("\u{f11c}", "Keybindings", run(&[&b("keys")])),
                item(
                    "\u{f0219}",
                    "Config files",
                    Action::Sub(
                        configs
                            .iter()
                            .filter(|(_, rel)| cfg.join(rel).exists())
                            .map(|(text, rel)| {
                                item("\u{f0219}", text, edit(&cfg.join(rel).to_string_lossy()))
                            })
                            .collect(),
                    ),
                ),
                item(
                    "\u{f062e}",
                    "GNOME Settings",
                    run(&["env", "XDG_CURRENT_DESKTOP=GNOME", "gnome-control-center"]),
                ),
            ]),
        ),
        item("\u{f0ab9}", "Install", Action::Sub(package("install"))),
        item("\u{f0a7a}", "Remove", Action::Sub(package("remove"))),
        item("\u{f021}", "Update", Action::Sub(updates)),
        item("\u{ea74}", "About", Action::Lazy(info_rows)),
        item(
            "\u{f011}",
            "System",
            Action::Sub(vec![
                item("\u{f1104}", "Screensaver", run(&[&b("launch-screensaver")])),
                item("\u{f033e}", "Lock", run(&["loginctl", "lock-session"])),
                item("\u{f04b2}", "Suspend", run(&["systemctl", "suspend"])),
                item(
                    "\u{f0343}",
                    "Logout",
                    run(&["hyprctl", "dispatch", "hl.dsp.exit()"]),
                ),
                item("\u{f0709}", "Restart", run(&["systemctl", "reboot"])),
                item("\u{f011}", "Shutdown", run(&["systemctl", "poweroff"])),
            ]),
        ),
    ]
}

// ------------------------------------------------- submenus read on opening --

/// Trigger › Notifications: the recent ones mako keeps, newest first.
/// Picking one copies its text.
fn notification_items() -> Vec<Item> {
    let data = json::parse(&out("makoctl", &["history"])).unwrap_or(Value::Null);
    let history = data
        .get("data")
        .map(Value::as_array)
        .and_then(<[Value]>::first)
        .map(Value::as_array)
        .unwrap_or(&[]);
    let mut items = Vec::new();
    for n in history.iter().take(30) {
        let field = |k: &str| {
            n.get(k)
                .and_then(|f| f.get("data"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        };
        let (summary, body, app) = (field("summary"), field("body"), field("app-name"));
        if app == "nameplate-osd" {
            continue; // volume and brightness popups aren't news
        }
        let text = if body.is_empty() {
            summary.clone()
        } else {
            format!("{summary}\n{body}")
        };
        let title = [summary.as_str(), body.as_str(), "(empty)"]
            .into_iter()
            .find(|s| !s.is_empty())
            .unwrap_or_default();
        items.push(
            item(
                "\u{f009a}",
                title,
                run(&["sh", "-c", "printf %s \"$1\" | wl-copy", "sh", &text]),
            )
            .right(&app),
        );
    }
    if items.is_empty() {
        items.push(item("", "No notifications yet", Action::Info));
    }
    items
}

/// Trigger › Reminders: the ones waiting, plus set and clear (bin/reminder).
fn reminder_items() -> Vec<Item> {
    let rem = bin("reminder");
    let mut items = vec![item("\u{f0415}", "New reminder", run(&[&rem, "set"]))];
    for line in out(&rem, &["list", "--plain"]).lines() {
        let (when, text) = line.split_once('\t').unwrap_or((line, ""));
        if !text.is_empty() {
            items.push(item("\u{f0e1e}", text, run(&[&rem, "list"])).right(when));
        }
    }
    if items.len() > 1 {
        items.push(item("\u{f0a7a}", "Clear all", run(&[&rem, "clear"])));
    }
    items
}

/// Setup › Hardware, like Omarchy's: only what this computer has.
fn hardware_items() -> Vec<Item> {
    let hw = bin("hardware");
    let devices = json::parse(&out("hyprctl", &["-j", "devices"])).unwrap_or(Value::Null);
    let mut items = Vec::new();
    let mice = devices.get("mice").map_or(&[][..], Value::as_array);
    if mice
        .iter()
        .any(|m| m.str_of("name").to_lowercase().contains("touchpad"))
    {
        items.push(
            item("\u{f0ce0}", "Touchpad", run(&[&hw, "touchpad"]))
                .right(on_off(!state_flag("no-touchpad"))),
        );
    }
    if !devices
        .get("touch")
        .map_or(&[][..], Value::as_array)
        .is_empty()
    {
        items.push(
            item("\u{f0741}", "Touchscreen", run(&[&hw, "touchscreen"]))
                .right(on_off(!state_flag("no-touchscreen"))),
        );
    }
    let kbd = std::fs::read_dir("/sys/class/leds")
        .map(|d| {
            d.flatten()
                .any(|e| e.file_name().to_string_lossy().contains("kbd_backlight"))
        })
        .unwrap_or(false);
    if kbd {
        items.push(item("\u{f030c}", "Keyboard light", run(&[&hw, "kbd-light"])).stay());
    }
    items.push(item("\u{f0379}", "Graphics", run(&[&hw, "graphics"])));
    items.push(item("\u{f140b}", "Power profile", Action::Panel("power")));
    items
}

/// Appearance › Font: installed Nerd Fonts, the terminal size, fonts to download.
fn font_items() -> Vec<Item> {
    let font = bin("font");
    let now = out(&font, &[]);
    let (family, size) = now
        .split_once(", terminal size ")
        .unwrap_or((now.as_str(), ""));
    let mut items: Vec<Item> = out(&font, &["list"])
        .lines()
        .filter(|n| !n.is_empty())
        .map(|name| {
            item("\u{f0b34}", name, run(&[&font, "set", name])).right(if name == family {
                "current"
            } else {
                ""
            })
        })
        .collect();
    let sizes = (8..=13)
        .map(|n: i32| {
            let text = if n == 9 {
                format!("{n}  (Omarchy)")
            } else {
                n.to_string()
            };
            let current = size.trim() == n.to_string() || size.trim() == format!("{n}.0");
            item("\u{f0b34}", &text, run(&[&font, "size", &n.to_string()])).right(if current {
                "current"
            } else {
                ""
            })
        })
        .collect();
    items.push(item("\u{f0b34}", "Terminal size", Action::Sub(sizes)).right(size));
    let more: Vec<Item> = out(&font, &["more"])
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(archive, name)| {
            item(
                "\u{f01da}",
                name,
                in_terminal("Fonts", &format!("{font} install {archive}")),
            )
        })
        .collect();
    if !more.is_empty() {
        items.push(item("\u{f01da}", "Get more fonts", Action::Sub(more)));
    }
    items
}

/// Lock screen blur (bin/toggle lock-blur): off, light or strong.
fn lock_blur() -> &'static str {
    let passes = sys::read(sys::app_dir().join("lock-blur.conf"))
        .unwrap_or_default()
        .lines()
        .find_map(|l| {
            let rest = l
                .strip_prefix("$np_blur")?
                .trim_start()
                .strip_prefix('=')?
                .trim_start();
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            digits.parse::<u32>().ok()
        })
        .unwrap_or(3);
    match passes {
        0 => "off",
        1 => "light",
        _ => "strong",
    }
}

/// About: a few facts about this machine, read on the spot.
fn info_rows() -> Vec<Item> {
    let first = |path: &str, key: Option<&str>| -> String {
        let text = sys::read(path).unwrap_or_default();
        for line in text.lines() {
            match key {
                None => return line.trim().to_string(),
                Some(k) if line.starts_with(k) => {
                    let sep = if line.contains('=') { '=' } else { ':' };
                    return line
                        .split_once(sep)
                        .map_or("", |p| p.1)
                        .trim()
                        .trim_matches('"')
                        .to_string();
                }
                _ => {}
            }
        }
        String::new()
    };
    let mem_kb: Option<f64> = first("/proc/meminfo", Some("MemTotal"))
        .split_whitespace()
        .next()
        .and_then(|n| n.parse().ok());
    let up = first("/proc/uptime", None)
        .split_whitespace()
        .next()
        .and_then(|n| n.parse::<f64>().ok())
        .unwrap_or(0.0) as i64;
    // The free space you can use, like shutil.disk_usage: df's "avail".
    let disk = out("df", &["-B1", "--output=size,avail", "/"]);
    let disk: Vec<f64> = disk
        .lines()
        .nth(1)
        .unwrap_or("")
        .split_whitespace()
        .filter_map(|n| n.parse().ok())
        .collect();
    let hypr = out("hyprctl", &["version"]);
    let hypr = hypr
        .lines()
        .next()
        .map(|l| {
            l.strip_prefix("Hyprland")
                .unwrap_or(l)
                .trim_start()
                .split(" built")
                .next()
                .unwrap_or("")
                .to_string()
        })
        .unwrap_or_default();
    let cpu = first("/proc/cpuinfo", Some("model name"))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace("(R)", "")
        .replace("(TM)", "")
        .replace(" CPU", "");
    let facts = [
        ("System", first("/etc/os-release", Some("PRETTY_NAME"))),
        ("Computer", first("/sys/class/dmi/id/product_name", None)),
        ("Processor", cpu),
        (
            "Memory",
            mem_kb.map_or(String::new(), |kb| format!("{:.0} GB", kb / 1_048_576.0)),
        ),
        (
            "Disk",
            match disk.as_slice() {
                [total, free] => format!("{:.0} of {:.0} GB free", free / 1e9, total / 1e9),
                _ => String::new(),
            },
        ),
        ("Kernel", first("/proc/sys/kernel/osrelease", None)),
        ("Hyprland", hypr),
        ("Up for", format!("{}h {:02}m", up / 3600, up % 3600 / 60)),
    ];
    facts
        .into_iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| item("", k, Action::Info).right(&v))
        .collect()
}

/// Every item reachable from the top through fixed submenus, with where it
/// lives ("Trigger › Capture"), for searching.
fn everything(items: &[Item], path: &str, out: &mut Vec<(Item, String)>) {
    for it in items {
        if let Action::Sub(sub) = &it.action {
            let inner = if path.is_empty() {
                it.label.clone()
            } else {
                format!("{path} › {}", it.label)
            };
            everything(sub, &inner, out);
        }
        if !it.info() {
            out.push((it.clone(), path.to_string()));
        }
    }
}

/// "wifi" finds Wi-Fi, "shutdown" finds Shut down: every search word starts a
/// word of the item's place and name, or its name run together.
fn found(item: &Item, path: &str, wanted: &[String]) -> bool {
    let text = format!("{path} {}", item.label)
        .to_lowercase()
        .replace('-', "");
    let mut words: Vec<String> = text.split_whitespace().map(String::from).collect();
    words.push(item.label.to_lowercase().replace([' ', '-'], ""));
    wanted
        .iter()
        .all(|q| words.iter().any(|w| w.starts_with(q.as_str())))
}

// ------------------------------------------------------------------ panel --

pub struct Menu {
    root: gtk::Box,
    search: gtk::Entry,
    prompt: gtk::Label,
    list: gtk::Box,
    top: Vec<Item>,
    /// (title, items), the open submenu last.
    stack: RefCell<Vec<(String, Vec<Item>)>>,
    buttons: RefCell<Vec<(gtk::Button, Item)>>,
    selected: Cell<usize>,
}

impl Menu {
    pub fn new(route: Option<&str>) -> Rc<Self> {
        let root = vbox(10);
        let search = gtk::Entry::new();
        search.style_context().add_class("np-menu-search");
        // Walker's prompt stays on show while you type, but GTK hides a focused
        // entry's placeholder, so the prompt is a label over the entry.
        let prompt = label("", &["np-menu-prompt"]);
        prompt.set_margin_start(11);
        prompt.set_valign(gtk::Align::Center);
        let overlay = gtk::Overlay::new();
        overlay.add(&search);
        overlay.add_overlay(&prompt);
        overlay.set_overlay_pass_through(&prompt, true);
        root.pack_start(&overlay, false, false, 0);
        let list = vbox(0);
        root.pack_start(&list, false, false, 0);

        let top = tree();
        let mut stack = vec![("Go".to_string(), top.clone())];
        let mut items = top.clone();
        for part in route.unwrap_or("").to_lowercase().split('/') {
            let Some(found) = items
                .iter()
                .find(|i| i.label.to_lowercase() == part)
                .and_then(|i| Some((i.label.clone(), i.items()?)))
            else {
                break;
            };
            items = found.1.clone();
            stack.push(found);
        }
        let me = Rc::new(Menu {
            root,
            search,
            prompt,
            list,
            top,
            stack: RefCell::new(stack),
            buttons: RefCell::default(),
            selected: Cell::new(0),
        });
        let weak = Rc::downgrade(&me);
        me.search.connect_changed(move |_| {
            if let Some(me) = weak.upgrade() {
                me.fill();
            }
        });
        let weak = Rc::downgrade(&me);
        me.search.connect_activate(move |_| {
            if let Some(me) = weak.upgrade() {
                let chosen = me
                    .buttons
                    .borrow()
                    .get(me.selected.get())
                    .map(|(_, i)| i.clone());
                if let Some(it) = chosen {
                    me.choose(&it);
                }
            }
        });
        me.fill();
        me
    }

    fn fill(self: &Rc<Self>) {
        clear(&self.list);
        self.buttons.borrow_mut().clear();
        let query = self.search.text().trim().to_lowercase();
        let (title, items) = self.stack.borrow().last().cloned().unwrap_or_default();
        self.prompt.set_text(&format!("{title}…")); // "Go…", "Appearance…"
        self.prompt.set_visible(query.is_empty());
        let shown: Vec<(Item, String)> = if query.is_empty() {
            items
                .into_iter()
                .map(|i| {
                    let right = i.right.clone();
                    (i, right)
                })
                .collect()
        } else {
            let wanted: Vec<String> = query
                .replace('-', "")
                .split_whitespace()
                .map(String::from)
                .collect();
            let mut all = Vec::new();
            everything(&self.top, "", &mut all);
            all.into_iter()
                .filter(|(i, path)| found(i, path, &wanted))
                .take(MAX_RESULTS)
                .collect()
        };
        for (it, right) in &shown {
            if it.info() {
                // About: a plain line, not a button.
                let line = hbox(10);
                line.style_context().add_class("np-menu-info");
                line.set_margin_start(14);
                line.set_margin_end(14);
                line.set_margin_top(8);
                line.set_margin_bottom(8);
                line.pack_start(&label(&it.label, &["np-dim"]), false, false, 5);
                let r = label(right, &[]);
                r.set_ellipsize(gtk::pango::EllipsizeMode::End);
                r.set_max_width_chars(30);
                r.set_width_chars(10);
                r.set_xalign(1.0);
                line.pack_end(&r, true, true, 5);
                self.list.pack_start(&line, false, false, 0);
                continue;
            }
            // Searching: just where it lives, e.g. "Config files".
            let right = if query.is_empty() {
                right.as_str()
            } else {
                right.rsplit(" › ").next().unwrap_or("")
            };
            let weak = Rc::downgrade(self);
            let chosen = it.clone();
            let b = row(&it.icon, &it.label, right, move || {
                if let Some(me) = weak.upgrade() {
                    me.choose(&chosen);
                }
            });
            b.style_context().add_class("np-menu-item");
            let weak = Rc::downgrade(self);
            b.connect_enter_notify_event(move |w, _| {
                if let Some(me) = weak.upgrade() {
                    let i = me.buttons.borrow().iter().position(|(b, _)| b == w);
                    if let Some(i) = i {
                        me.select(i);
                    }
                }
                gtk::glib::Propagation::Proceed
            });
            self.buttons.borrow_mut().push((b.clone(), it.clone()));
            self.list.pack_start(&b, false, false, 0);
        }
        if shown.is_empty() {
            let text = if query.is_empty() {
                "Nothing here."
            } else {
                "No matches."
            };
            self.list
                .pack_start(&label(text, &["np-dim"]), false, false, 6);
        }
        self.list.show_all();
        self.select(0);
    }

    fn select(&self, i: usize) {
        let buttons = self.buttons.borrow();
        if buttons.is_empty() {
            return;
        }
        let i = i.min(buttons.len() - 1);
        self.selected.set(i);
        for (n, (b, _)) in buttons.iter().enumerate() {
            if n == i {
                b.style_context().add_class("np-selected");
            } else {
                b.style_context().remove_class("np-selected");
            }
        }
    }

    fn choose(self: &Rc<Self>, it: &Item) {
        if let Some(items) = it.items() {
            self.stack.borrow_mut().push((it.label.clone(), items));
            self.search.set_text("");
            self.fill();
            self.search.grab_focus();
            return;
        }
        match &it.action {
            Action::Panel(kind) => {
                super::close(); // frees the panel slot first
                super::switch_to(kind);
            }
            Action::Run(cmd) => {
                let args: Vec<&str> = cmd.iter().map(String::as_str).collect();
                if it.stay {
                    if let Some((prog, rest)) = args.split_first() {
                        sys::spawn(prog, rest);
                    }
                    return;
                }
                super::close();
                // A moment for the menu to go, so screenshots and the lock don't catch it.
                let mut all = vec!["-c", "sleep 0.25; exec \"$@\"", "sh"];
                all.extend(args);
                sys::spawn("sh", &all);
            }
            _ => {}
        }
    }

    fn go_back(self: &Rc<Self>) -> bool {
        if self.stack.borrow().len() > 1 {
            self.stack.borrow_mut().pop();
            self.search.set_text("");
            self.fill();
            self.search.grab_focus();
            true
        } else {
            false
        }
    }

    fn has_sub(&self) -> bool {
        let buttons = self.buttons.borrow();
        buttons
            .get(self.selected.get())
            .is_some_and(|(_, i)| matches!(i.action, Action::Sub(_) | Action::Lazy(_)))
    }
}

impl Panel for Menu {
    fn root(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }
    fn width(&self) -> i32 {
        275 // 315px with the padding: Walker's --width 295 box
    }
    fn anchor(&self) -> Anchor {
        Anchor::Middle
    }
    fn padding(&self) -> u32 {
        20 // Walker's .box-wrapper padding
    }
    fn window_class(&self) -> Option<&'static str> {
        Some("np-menu")
    }
    fn shown(self: Rc<Self>) {
        self.search.grab_focus();
    }
    fn on_key(self: Rc<Self>, event: &gdk::EventKey) -> bool {
        use gdk::keys::constants as key;
        let me = self;
        let k = event.keyval();
        let empty = me.search.text().is_empty();
        if k == key::Escape {
            if !empty {
                me.search.set_text("");
                return true;
            }
            return me.go_back(); // at the top, Escape closes as usual
        }
        if k == key::BackSpace && empty {
            me.go_back();
            return true;
        }
        let count = me.buttons.borrow().len();
        if [key::Down, key::Tab, key::Up, key::ISO_Left_Tab].contains(&k) && count > 0 {
            let step = if k == key::Down || k == key::Tab {
                1
            } else {
                count - 1
            };
            me.select((me.selected.get() + step) % count); // wraps around
            return true;
        }
        if k == key::Right && count > 0 && me.has_sub() && empty {
            let chosen = me
                .buttons
                .borrow()
                .get(me.selected.get())
                .map(|(_, i)| i.clone());
            if let Some(it) = chosen {
                me.choose(&it);
            }
            return true;
        }
        if k == key::Left && empty {
            return me.go_back();
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_words() {
        let wifi = item("x", "Wi-Fi", Action::Panel("wifi"));
        let off = item("x", "Shutdown", Action::Info);
        let w = |q: &str| q.split_whitespace().map(String::from).collect::<Vec<_>>();
        assert!(found(&wifi, "Setup", &w("wifi")));
        assert!(found(&wifi, "Setup", &w("set wi")));
        assert!(!found(&wifi, "Setup", &w("fi")));
        assert!(found(&off, "System", &w("shut")));
        let tree = vec![item(
            "a",
            "Setup",
            Action::Sub(vec![wifi.clone(), item("i", "Info", Action::Info)]),
        )];
        let mut all = Vec::new();
        everything(&tree, "", &mut all);
        let names: Vec<(String, String)> = all
            .iter()
            .map(|(i, p)| (i.label.clone(), p.clone()))
            .collect();
        assert_eq!(
            names,
            [
                ("Wi-Fi".into(), "Setup".into()),
                ("Setup".into(), "".into())
            ]
        );
    }
}
