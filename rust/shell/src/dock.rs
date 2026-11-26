//! The dock along the bottom, like Omarchy 4's: the app grid button, pinned apps
//! and whatever else is open, as large icons floating over the desktop.
//!
//! It lives on floating workspaces (SUPER + L). A maximized window tucks it to
//! the bottom edge until the pointer touches there, a fullscreen one hides it,
//! and right-clicking the grid button can make it always tuck away. Click an app
//! to open it or jump to its window (again for the next one), drag icons to
//! reorder, right-click for new window / pin / quit. Minimized windows
//! (SUPER + M) wait here too.
//!
//! The command line (`pin`, `unpin`, `pinned`, `autohide`) edits
//! ~/.config/nameplate/dock.json and pokes the running dock through its
//! pidfile: SIGUSR1 to re-read pins and apps, SIGUSR2 for layout/autohide.
//!
//! Hyprland is read over its socket on a worker thread, because it can stall
//! for seconds at login; answers are applied on the GTK thread and stale ones
//! dropped. Several workarounds below exist for one failure: a layer surface
//! that Hyprland stops sending pointer events to after the monitor is
//! reconfigured at login. They're kept from the Python version, where each was
//! found the hard way.

use crate::util;
use gtk::prelude::*;
use gtk::{cairo, gdk, gdk_pixbuf::Pixbuf, gio, glib};
use gtk_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use nameplate::json::{self, Value};
use nameplate::{ffi, sys};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

const ICON: i32 = 36;
const INSET: i32 = 6;
const GAP: i32 = 3;
const SLOT: i32 = ICON + INSET * 2;
const HOVER_ICON: i32 = 41; // ICON * 1.15
/// The strip left on screen while the dock is tucked away.
const HOTSPOT: i32 = 12;
const DRAG_TARGET: &str = "application/x-nameplate-dock";

/// Pinned to start with: the first installed app of each group.
const DEFAULT_PINS: &[&[&str]] = &[
    &[
        "firefox_firefox.desktop",
        "firefox.desktop",
        "google-chrome.desktop",
        "chromium_chromium.desktop",
        "brave-browser.desktop",
    ],
    &[
        "Alacritty.desktop",
        "alacritty.desktop",
        "org.gnome.Terminal.desktop",
    ],
    &["org.gnome.Nautilus.desktop", "nautilus.desktop"],
    &["code.desktop", "code_code.desktop"],
    &["spotify_spotify.desktop", "spotify.desktop"],
    &[
        "notion-snap-reborn_notion-snap-reborn.desktop",
        "notion-app.desktop",
    ],
];

thread_local! {
    static DOCK: RefCell<Option<Rc<Dock>>> = const { RefCell::new(None) };
}

/// Reach the dock from a closure posted by a worker thread.
fn with_dock(f: impl FnOnce(&Rc<Dock>)) {
    let dock = DOCK.with(|d| d.borrow().clone());
    if let Some(dock) = dock {
        f(&dock);
    }
}

// ------------------------------------------------------------------ config --

const PIDFILE: &str = "nameplate-dock.pid";
const MARKERS: &[&str] = &["nameplate-shell dock"];

fn config_path() -> PathBuf {
    sys::app_dir().join("dock.json")
}

fn load_config() -> BTreeMap<String, Value> {
    match sys::read(config_path()).and_then(|t| json::parse(&t)) {
        Some(Value::Object(map)) => map,
        _ => BTreeMap::new(),
    }
}

/// Change some keys of dock.json, keeping the rest.
fn save_config(update: impl FnOnce(&mut BTreeMap<String, Value>)) {
    let mut map = load_config();
    update(&mut map);
    let _ = sys::write_atomic(config_path(), &format!("{}\n", Value::Object(map)));
}

fn load_pins() -> Option<Vec<String>> {
    match load_config().get("pinned") {
        Some(Value::Array(items)) => Some(
            items
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
        ),
        _ => None,
    }
}

fn save_pins(pins: &[String]) {
    let list = Value::Array(pins.iter().cloned().map(Value::Str).collect());
    save_config(|m| {
        m.insert("pinned".into(), list);
    });
}

fn load_autohide() -> bool {
    load_config().get("autohide").and_then(Value::as_bool) == Some(true)
}

// -------------------------------------------------------------------- apps --

/// Installed apps, and which one a window belongs to.
struct Apps {
    all: HashMap<String, gio::AppInfo>,
    by_class: HashMap<String, String>,
}

impl Apps {
    fn load() -> Apps {
        let mut all = HashMap::new();
        for app in gio::AppInfo::all() {
            if let Some(id) = app.id() {
                all.insert(id.to_string(), app);
            }
        }
        let mut by_class: HashMap<String, String> = HashMap::new();
        for (id, app) in &all {
            let stem = id.trim_end_matches(".desktop").to_lowercase();
            let mut keys = vec![
                stem.clone(),
                stem.rsplit('_').next().unwrap_or("").to_string(),
                stem.rsplit('.').next().unwrap_or("").to_string(),
            ];
            if let Some(wm) = app
                .downcast_ref::<gio::DesktopAppInfo>()
                .and_then(|d| d.startup_wm_class())
            {
                keys.push(wm.to_lowercase());
            }
            if let Some(name) = app.executable().file_name() {
                keys.push(name.to_string_lossy().to_lowercase());
            }
            for key in keys.into_iter().filter(|k| !k.is_empty()) {
                if !by_class.contains_key(&key) || app.should_show() {
                    by_class.insert(key, id.clone());
                }
            }
        }
        Apps { all, by_class }
    }

    fn for_window(&self, c: &Client) -> Option<String> {
        for name in [&c.class, &c.initial_class] {
            let lower = name.to_lowercase();
            if lower.is_empty() {
                continue;
            }
            for key in [
                lower.clone(),
                lower.rsplit('.').next().unwrap_or("").to_string(),
                lower.replace(' ', "-"),
            ] {
                if let Some(id) = self.by_class.get(&key) {
                    return Some(id.clone());
                }
            }
        }
        None
    }
}

fn default_pins(apps: &Apps) -> Vec<String> {
    DEFAULT_PINS
        .iter()
        .filter_map(|group| {
            group
                .iter()
                .find(|id| apps.all.contains_key(**id))
                .map(|id| id.to_string())
        })
        .collect()
}

fn launch(app_id: &str) {
    sys::spawn("gtk-launch", &[app_id]);
}

// ---------------------------------------------------------- Hyprland state --

#[derive(Clone, Debug)]
struct Client {
    address: String,
    class: String,
    initial_class: String,
    workspace_id: i64,
    workspace_name: String,
    fullscreen: i64,
    focus_history: i64,
    tags: Vec<String>,
    minimized: bool,
}

impl Client {
    fn parse(v: &Value) -> Option<Client> {
        if v.get("mapped").and_then(Value::as_bool) == Some(false)
            || v.get("hidden").and_then(Value::as_bool) == Some(true)
        {
            return None;
        }
        let ws = v.get("workspace")?;
        let num = |x: Option<&Value>| x.and_then(Value::as_f64).unwrap_or(0.0) as i64;
        Some(Client {
            address: v.str_of("address").to_string(),
            class: v.str_of("class").to_string(),
            initial_class: v.str_of("initialClass").to_string(),
            workspace_id: ws.get("id").and_then(Value::as_f64).map_or(1, |n| n as i64),
            workspace_name: ws.str_of("name").to_string(),
            fullscreen: num(v.get("fullscreen")),
            focus_history: num(v.get("focusHistoryID")),
            tags: v
                .get("tags")
                .map(|t| {
                    t.as_array()
                        .iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            minimized: false,
        })
    }

    /// The workspace a minimized window came from (bin/window-minimize tags it).
    fn minimized_from(&self) -> Option<i64> {
        self.tags
            .iter()
            .find_map(|t| t.strip_prefix("nameplate-minimized-from-")?.parse().ok())
    }
}

/// One reading of Hyprland, taken off the GTK thread.
struct Reading {
    clients: Option<Value>,
    workspace: Option<Value>,
    active: Option<Value>,
}

fn read_hyprland() -> Reading {
    Reading {
        clients: sys::hypr_json("clients"),
        workspace: sys::hypr_json("activeworkspace"),
        active: sys::hypr_json("activewindow"),
    }
}

/// The active workspace's layout, from `nameplate layout`'s state files,
/// falling back to the tiled layout Hyprland reports.
fn workspace_mode(ws: &Value) -> String {
    let id = ws.get("id").and_then(Value::as_f64).map(|n| n as i64);
    if let Some(id) = id {
        if let Some(mode) = sys::read(
            sys::state_dir()
                .join("workspace-layouts")
                .join(id.to_string()),
        ) {
            if matches!(mode.as_str(), "dwindle" | "scrolling" | "floating") {
                return mode;
            }
        }
    }
    match ws.str_of("tiledLayout") {
        "" => "dwindle".into(),
        other => other.into(),
    }
}

fn monitor_fingerprint() -> Option<String> {
    let mons = sys::hypr_json("monitors")?;
    let mut parts: Vec<String> = mons
        .as_array()
        .iter()
        .filter(|m| m.get("disabled").and_then(Value::as_bool) != Some(true))
        .map(|m| {
            let n = |k: &str| m.get(k).and_then(Value::as_f64).unwrap_or(0.0);
            format!(
                "{} {}x{} {} {},{}",
                m.str_of("name"),
                n("width"),
                n("height"),
                n("scale"),
                n("x"),
                n("y")
            )
        })
        .collect();
    parts.sort();
    Some(parts.join("; "))
}

/// Our own layer surface as Hyprland sees it: Ok(Some(height)), Ok(None) when it
/// has none, Err when Hyprland didn't answer (which must not look like "missing").
fn our_layer_height() -> Result<Option<i64>, ()> {
    let layers = sys::hypr_json("layers").ok_or(())?;
    let pid = std::process::id() as f64;
    fn walk(v: &Value, pid: f64) -> Option<i64> {
        match v {
            Value::Object(map) => {
                if v.str_of("namespace") == "nameplate-dock"
                    && v.get("pid").and_then(Value::as_f64) == Some(pid)
                {
                    return Some(v.get("h").and_then(Value::as_f64).unwrap_or(0.0) as i64);
                }
                map.values().find_map(|x| walk(x, pid))
            }
            Value::Array(items) => items.iter().find_map(|x| walk(x, pid)),
            _ => None,
        }
    }
    Ok(walk(&layers, pid))
}

// -------------------------------------------------------------------- dock --

struct Widgets {
    window: gtk::Window,
    row: gtk::Box,
}

#[derive(Default)]
struct State {
    pins: Vec<String>,
    windows: HashMap<String, Vec<Client>>, // app id -> its windows, minimized ones included
    active: Option<String>,
    workspace_id: Option<i64>,
    floating: bool,
    fs_mode: i64, // 0 none, 1 maximized, 2+ fullscreen
    autohide: bool,
    revealed: bool,
    menu_open: bool,
    order: Vec<String>, // this session's drag order
    shown: Vec<String>,
    cycle: HashMap<String, usize>,
    margin: Option<i32>,
    seen_height: Option<i32>,
    height: i32,
    monitor_sig: Option<String>,
    rebuilt_at: Option<Instant>,
    state_sig: Option<String>,
    seq: u64,
    refresh_pending: bool,
}

pub struct Dock {
    apps: RefCell<Apps>,
    ui: RefCell<Option<Widgets>>,
    st: RefCell<State>,
    hide_timer: RefCell<Option<glib::SourceId>>,
    fg: (f64, f64, f64),
}

impl Dock {
    fn new() -> Rc<Dock> {
        let apps = Apps::load();
        let pins = load_pins().unwrap_or_else(|| {
            let pins = default_pins(&apps);
            save_pins(&pins);
            pins
        });
        let dock = Rc::new(Dock {
            apps: RefCell::new(apps),
            ui: RefCell::new(None),
            st: RefCell::new(State {
                pins,
                autohide: load_autohide(),
                height: SLOT + GAP + HOTSPOT,
                ..State::default()
            }),
            hide_timer: RefCell::new(None),
            fg: sys::rgb(&sys::theme_color("foreground")),
        });
        dock.css();
        // bin/display restore reconfigures the monitor in the same login burst;
        // a surface mapped during that never sees the pointer again.
        dock.wait_until_settled();
        dock.build();
        let seq = dock.st.borrow().seq;
        dock.apply(seq, read_hyprland()); // the first frame is already right
        dock.show();
        dock.st.borrow_mut().rebuilt_at = Some(Instant::now());
        dock
    }

    fn css(&self) {
        let c = |k: &str| sys::theme_color(k);
        util::add_css(&format!(
            "window.np-dock {{ background-color: transparent; }}
.np-dock .np-hot {{ background-color: rgba(0,0,0,0.01); }}
.np-dock button {{ background-image: none; background-color: transparent; border: none; box-shadow: none;
  padding: 0; margin: 0; min-width: 0; min-height: 0; outline: none; }}
.np-dock image {{ -gtk-icon-shadow: 0 2px 6px rgba(0,0,0,0.55); }}
.np-dock .np-dot {{ background-color: {fg}; min-width: 4px; min-height: 4px; border-radius: 2px; }}
.np-dock .np-dot.np-focused {{ background-color: {accent}; min-width: 12px; }}
.np-dock menu, .np-dock .menu {{ background-color: {bg}; border: 2px solid {accent}; padding: 4px; }}
.np-dock menuitem {{ color: {fg}; padding: 6px 12px; font-family: \"JetBrainsMono Nerd Font\", monospace; }}
.np-dock menuitem:hover {{ background-color: {sel}; }}",
            fg = c("foreground"),
            accent = c("accent"),
            bg = c("background"),
            sel = c("selection"),
        ));
    }

    // -- the window

    fn build(self: &Rc<Self>) {
        let height = self.st.borrow().height;
        let window = gtk::Window::new(gtk::WindowType::Toplevel);
        window.style_context().add_class("np-dock");
        // Never zero-size: a collapsed surface is ignored by the compositor
        // until the process restarts.
        window.set_size_request(240, height);
        util::transparent(&window);
        window.init_layer_shell();
        window.set_namespace("nameplate-dock");
        window.set_layer(Layer::Top);
        window.set_anchor(Edge::Bottom, true);
        window.set_exclusive_zone(0);
        window.set_keyboard_mode(KeyboardMode::None);

        let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        // Tucking slides the whole window down (negative bottom margin), so
        // the hotspot has to be its top edge: that's what stays on screen.
        let edge = gtk::EventBox::new();
        edge.set_size_request(-1, HOTSPOT);
        edge.style_context().add_class("np-hot");
        let me = Rc::downgrade(self);
        edge.connect_enter_notify_event(move |_, _| {
            if let Some(d) = me.upgrade() {
                d.reveal(true);
            }
            glib::Propagation::Proceed
        });
        outer.pack_start(&edge, false, false, 0);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        row.set_margin_bottom(GAP);
        outer.pack_start(&row, false, false, 0);

        let hover = gtk::EventBox::new();
        hover.add(&outer);
        hover.add_events(gdk::EventMask::ENTER_NOTIFY_MASK | gdk::EventMask::LEAVE_NOTIFY_MASK);
        let me = Rc::downgrade(self);
        hover.connect_enter_notify_event(move |_, _| {
            if let Some(d) = me.upgrade() {
                d.reveal(true);
            }
            glib::Propagation::Proceed
        });
        let me = Rc::downgrade(self);
        hover.connect_leave_notify_event(move |_, ev| {
            if let Some(d) = me.upgrade() {
                if ev.detail() != gdk::NotifyType::Inferior {
                    d.schedule_hide(); // still over the dock otherwise, just onto an icon
                }
            }
            glib::Propagation::Proceed
        });
        window.add(&hover);
        let me = Rc::downgrade(self);
        window.connect_size_allocate(move |w, _| {
            if let Some(d) = me.upgrade() {
                d.on_size(w);
            }
        });
        {
            let mut st = self.st.borrow_mut();
            st.seen_height = None;
            st.margin = None;
        }
        *self.ui.borrow_mut() = Some(Widgets { window, row });
    }

    fn show(&self) {
        if let Some(ui) = self.ui.borrow().as_ref() {
            ui.window.show_all();
        }
    }

    fn on_size(self: &Rc<Self>, window: &gtk::Window) {
        let (w, h) = (window.allocated_width(), window.allocated_height());
        {
            let mut st = self.st.borrow_mut();
            if h < HOTSPOT + 10 || st.seen_height == Some(h) {
                return;
            }
            st.seen_height = Some(h);
            st.height = h;
            st.margin = None; // the first margin was a guess from the size request
        }
        if let Some(gdk_window) = window.window() {
            if w > 1 {
                // A fully transparent strip can get an empty input region; claim
                // the whole window so the hotspot keeps receiving the pointer.
                let region = cairo::Region::create_rectangle(&cairo::RectangleInt::new(0, 0, w, h));
                gdk_window.input_shape_combine_region(&region, 0, 0);
            }
        }
        let me = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            if let Some(d) = me.upgrade() {
                d.update();
            }
        });
    }

    /// Omarchy's show model: on floating workspaces only; tucked to the edge
    /// with autohide on or a maximized window; hidden for fullscreen. The
    /// surface keeps its full height either way, so the pointer still reaches it.
    fn update(&self) {
        let ui = self.ui.borrow();
        let Some(ui) = ui.as_ref() else { return };
        let mut st = self.st.borrow_mut();
        let visible = st.floating && st.fs_mode < 2;
        let tucked = visible && (st.autohide || st.fs_mode == 1) && !(st.revealed || st.menu_open);
        let mut height = ui.window.allocated_height();
        if height < HOTSPOT + 20 {
            height = st.height;
        }
        let margin = if !visible {
            -(height + 80)
        } else if tucked {
            -(height - HOTSPOT)
        } else {
            0
        };
        if st.margin != Some(margin) {
            st.margin = Some(margin);
            ui.window.set_layer_shell_margin(Edge::Bottom, margin);
        }
    }

    fn rebuild(self: &Rc<Self>) {
        {
            let mut st = self.st.borrow_mut();
            st.seq += 1;
            st.rebuilt_at = Some(Instant::now());
            st.state_sig = None;
        }
        if let Some(old) = self.ui.borrow_mut().take() {
            unsafe { old.window.destroy() };
        }
        self.build();
        self.fill();
        self.update();
        self.show();
    }

    // -- the icons

    fn fill(self: &Rc<Self>) {
        let row = match self.ui.borrow().as_ref() {
            Some(ui) => ui.row.clone(),
            None => return,
        };
        for child in row.children() {
            row.remove(&child);
        }
        let grid = self.slot(self.grid_icon().upcast(), None);
        grid.set_tooltip_text(Some("All apps"));
        grid.connect_clicked(|_| sys::spawn(sys::bin("panel"), &["apps"]));
        let me = Rc::downgrade(self);
        grid.connect_button_press_event(move |_, ev| {
            if ev.button() == 3 {
                if let Some(d) = me.upgrade() {
                    d.dock_menu(ev);
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        row.pack_start(&grid, false, false, 0);

        let shown = {
            let apps = self.apps.borrow();
            let st = self.st.borrow();
            let mut shown: Vec<String> = st
                .pins
                .iter()
                .filter(|p| apps.all.contains_key(*p))
                .cloned()
                .collect();
            let mut open: Vec<&String> = st.windows.keys().filter(|a| !shown.contains(a)).collect();
            open.sort();
            shown.extend(open.into_iter().cloned());
            if !st.order.is_empty() {
                let rank: HashMap<&String, usize> =
                    st.order.iter().enumerate().map(|(i, a)| (a, i)).collect();
                shown.sort_by_key(|a| rank.get(a).copied().unwrap_or(rank.len()));
            }
            shown
        };
        self.st.borrow_mut().shown = shown.clone();
        for app_id in shown {
            row.pack_start(&self.app_slot(&app_id), false, false, 0);
        }
        row.show_all();
    }

    fn app_slot(self: &Rc<Self>, app_id: &str) -> gtk::Button {
        let app = self.apps.borrow().all.get(app_id).cloned();
        let (mark, focused) = {
            let st = self.st.borrow();
            let clients = st.windows.get(app_id);
            let focused = clients.is_some_and(|cs| {
                cs.iter()
                    .any(|c| !c.minimized && Some(&c.address) == st.active.as_ref())
            });
            (clients.is_some_and(|cs| !cs.is_empty()), focused)
        };
        let (image, small, big) = icon_for(app.as_ref());
        let button = self.slot(image.clone().upcast(), mark.then_some(focused));
        button.set_tooltip_text(Some(
            &app.as_ref()
                .map(|a| a.display_name().to_string())
                .unwrap_or_else(|| app_id.to_string()),
        ));

        let dragging = Rc::new(Cell::new(false));
        let me = Rc::downgrade(self);
        let id = app_id.to_string();
        let drag = dragging.clone();
        button.connect_clicked(move |_| {
            if drag.get() {
                return; // a drag ended here; not a click
            }
            if let Some(d) = me.upgrade() {
                d.activate(&id);
            }
        });
        let me = Rc::downgrade(self);
        let id = app_id.to_string();
        button.connect_button_press_event(move |_, ev| {
            if ev.button() == 3 {
                if let Some(d) = me.upgrade() {
                    d.app_menu(&id, ev);
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        if let (Some(small), Some(big)) = (small, big) {
            let img = image.clone();
            button.connect_enter_notify_event(move |_, _| {
                img.set_from_pixbuf(Some(&big));
                glib::Propagation::Proceed
            });
            let img = image;
            button.connect_leave_notify_event(move |_, _| {
                img.set_from_pixbuf(Some(&small));
                glib::Propagation::Proceed
            });
        }
        self.make_draggable(&button, app_id, dragging);
        button
    }

    /// A slot: `child` centred, with a dot under it for an open app
    /// (`Some(false)`) or a wider accent bar for the focused one (`Some(true)`).
    fn slot(&self, child: gtk::Widget, mark: Option<bool>) -> gtk::Button {
        let button = gtk::Button::new();
        button.set_size_request(SLOT, SLOT);
        let holder = gtk::Box::new(gtk::Orientation::Vertical, 0);
        holder.set_size_request(SLOT, SLOT);
        let icon_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        icon_box.set_size_request(SLOT, if mark.is_some() { SLOT - 4 } else { SLOT });
        icon_box.set_center_widget(Some(&child));
        holder.pack_start(&icon_box, false, false, 0);
        if let Some(focused) = mark {
            let marks = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            marks.set_halign(gtk::Align::Center);
            marks.set_size_request(-1, 4);
            let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            dot.style_context().add_class("np-dot");
            if focused {
                dot.style_context().add_class("np-focused");
            }
            marks.pack_start(&dot, false, false, 0);
            holder.pack_start(&marks, false, false, 0);
        }
        button.add(&holder);
        button
    }

    /// Omarchy's app grid mark: nine rounded squares.
    fn grid_icon(&self) -> gtk::DrawingArea {
        let area = gtk::DrawingArea::new();
        area.set_size_request(ICON, ICON);
        area.set_valign(gtk::Align::Center);
        area.set_halign(gtk::Align::Center);
        let (r, g, b) = self.fg;
        area.connect_draw(move |_, cr| {
            let sq = (ICON as f64 * 0.22).round();
            let gap = (ICON as f64 * 0.1).round();
            let off = (ICON as f64 - (3.0 * sq + 2.0 * gap)) / 2.0;
            let radius = (sq * 0.2).round().max(1.0);
            cr.set_source_rgb(r, g, b);
            for row in 0..3 {
                for col in 0..3 {
                    let x = off + col as f64 * (sq + gap);
                    let y = off + row as f64 * (sq + gap);
                    cr.new_sub_path();
                    cr.arc(
                        x + sq - radius,
                        y + radius,
                        radius,
                        -std::f64::consts::FRAC_PI_2,
                        0.0,
                    );
                    cr.arc(
                        x + sq - radius,
                        y + sq - radius,
                        radius,
                        0.0,
                        std::f64::consts::FRAC_PI_2,
                    );
                    cr.arc(
                        x + radius,
                        y + sq - radius,
                        radius,
                        std::f64::consts::FRAC_PI_2,
                        std::f64::consts::PI,
                    );
                    cr.arc(
                        x + radius,
                        y + radius,
                        radius,
                        std::f64::consts::PI,
                        1.5 * std::f64::consts::PI,
                    );
                    cr.close_path();
                    let _ = cr.fill();
                }
            }
            glib::Propagation::Stop
        });
        area
    }

    /// Drag an icon along the dock to reorder it. GTK only starts a drag past
    /// its threshold, so a plain click still activates.
    fn make_draggable(
        self: &Rc<Self>,
        button: &gtk::Button,
        app_id: &str,
        dragging: Rc<Cell<bool>>,
    ) {
        let targets = [gtk::TargetEntry::new(
            DRAG_TARGET,
            gtk::TargetFlags::SAME_APP,
            0,
        )];
        button.drag_source_set(
            gdk::ModifierType::BUTTON1_MASK,
            &targets,
            gdk::DragAction::MOVE,
        );
        button.drag_dest_set(gtk::DestDefaults::ALL, &targets, gdk::DragAction::MOVE);
        let d = dragging.clone();
        button.connect_drag_begin(move |_, _| d.set(true));
        button.connect_drag_end(move |_, _| {
            // Clear after the release that ends the drag, so it isn't a click.
            let d = dragging.clone();
            glib::timeout_add_local_once(Duration::from_millis(300), move || d.set(false));
        });
        let id = app_id.to_string();
        button.connect_drag_data_get(move |_, _, selection, _, _| {
            selection.set(&selection.target(), 8, id.as_bytes());
        });
        let me = Rc::downgrade(self);
        let target = app_id.to_string();
        button.connect_drag_data_received(move |_, _, _, _, selection, _, _| {
            let key = String::from_utf8_lossy(&selection.data()).to_string();
            if key.is_empty() {
                return;
            }
            let me = me.clone();
            let target = target.clone();
            // After the drop: rebuilding now would destroy the drag's own buttons.
            glib::idle_add_local_once(move || {
                if let Some(d) = me.upgrade() {
                    d.place(&key, &target);
                }
            });
        });
    }

    /// Put `key` where `target` is. Session order, except that pins are saved.
    fn place(self: &Rc<Self>, key: &str, target: &str) {
        {
            let mut st = self.st.borrow_mut();
            let mut keys = st.shown.clone();
            let (Some(from), true) = (
                keys.iter().position(|k| k == key),
                keys.iter().any(|k| k == target),
            ) else {
                return;
            };
            if key == target {
                return;
            }
            let moved = keys.remove(from);
            let to = keys.iter().position(|k| k == target).unwrap_or(keys.len());
            keys.insert(to, moved);
            let pins: Vec<String> = keys
                .iter()
                .filter(|k| st.pins.contains(k))
                .cloned()
                .collect();
            let same_set =
                pins.iter().collect::<HashSet<_>>() == st.pins.iter().collect::<HashSet<_>>();
            if same_set && pins != st.pins {
                st.pins = pins;
                save_pins(&st.pins);
            }
            st.order = keys;
        }
        self.fill();
    }

    // -- state from Hyprland

    /// Re-read Hyprland on a worker thread; the answer lands on the GTK thread
    /// and is dropped if a newer read was started meanwhile.
    fn refresh(&self) {
        let seq = {
            let mut st = self.st.borrow_mut();
            st.seq += 1;
            st.seq
        };
        std::thread::spawn(move || {
            let reading = read_hyprland();
            util::on_main(move || with_dock(|d| d.apply(seq, reading)));
        });
    }

    /// Coalesce a burst of Hyprland events into one refresh.
    fn schedule_refresh(self: &Rc<Self>) {
        if std::mem::replace(&mut self.st.borrow_mut().refresh_pending, true) {
            return;
        }
        let me = Rc::downgrade(self);
        glib::timeout_add_local_once(Duration::from_millis(150), move || {
            if let Some(d) = me.upgrade() {
                d.st.borrow_mut().refresh_pending = false;
                d.refresh();
            }
        });
    }

    fn apply(self: &Rc<Self>, seq: u64, reading: Reading) {
        if seq != self.st.borrow().seq {
            return;
        }
        // No answer (common in the first moments of login): keep the last
        // decision rather than guess "no windows" or "windows".
        let Some(ws) = reading.workspace.filter(|w| w.get("id").is_some()) else {
            let empty = self
                .ui
                .borrow()
                .as_ref()
                .map_or(true, |ui| ui.row.children().is_empty());
            if empty {
                self.fill();
                self.update();
            }
            return;
        };
        let active = reading
            .active
            .as_ref()
            .map(|a| a.str_of("address").to_string())
            .filter(|a| !a.is_empty());
        let wid = ws.get("id").and_then(Value::as_f64).map(|n| n as i64);
        let mode = workspace_mode(&ws);

        let mut windows: HashMap<String, Vec<Client>> = HashMap::new();
        let mut fs_mode = 0;
        {
            let apps = self.apps.borrow();
            for raw in reading
                .clients
                .as_ref()
                .map(|c| c.as_array())
                .unwrap_or(&[])
            {
                let Some(mut c) = Client::parse(raw) else {
                    continue;
                };
                let Some(app_id) = apps.for_window(&c) else {
                    continue;
                };
                if c.workspace_name == "special:minimized" {
                    c.minimized = true;
                } else if c.workspace_id < 0 {
                    continue; // other special workspaces
                } else if wid.map_or(true, |w| w == c.workspace_id) {
                    fs_mode = fs_mode.max(c.fullscreen);
                }
                windows.entry(app_id).or_default().push(c);
            }
        }

        // Title ticks (a spinning terminal title) send activewindow over and
        // over; only rebuild the icons when something we draw changed.
        let mut sig_parts: Vec<String> = windows
            .iter()
            .map(|(app, cs)| {
                let mut addrs: Vec<String> = cs
                    .iter()
                    .map(|c| {
                        format!(
                            "{}{}{}",
                            c.address,
                            if c.minimized { "m" } else { "" },
                            c.minimized_from().unwrap_or(0)
                        )
                    })
                    .collect();
                addrs.sort();
                format!("{app}={}", addrs.join(","))
            })
            .collect();
        sig_parts.sort();
        let sig = format!("{:?}|{mode}|{fs_mode}|{}", active, sig_parts.join(";"));

        let changed = {
            let mut st = self.st.borrow_mut();
            st.active = active;
            st.workspace_id = wid;
            st.floating = mode == "floating";
            st.fs_mode = fs_mode;
            st.windows = windows;
            if st.state_sig.as_deref() == Some(sig.as_str()) {
                false
            } else {
                st.state_sig = Some(sig);
                true
            }
        };
        if changed {
            self.fill();
            self.update();
        }
    }

    fn wait_until_settled(&self) {
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut last = None;
        let mut stable = 0;
        while Instant::now() < deadline && stable < 2 {
            let sig = monitor_fingerprint();
            let ready = sys::hypr_json("activeworkspace").is_some_and(|w| w.get("id").is_some());
            if sig.is_some() && sig == last && ready {
                stable += 1;
            } else {
                stable = 0;
                last = sig;
            }
            if stable < 2 {
                std::thread::sleep(Duration::from_millis(350));
            }
        }
        self.st.borrow_mut().monitor_sig = last;
    }

    /// Every few seconds: rebuild the surface if the monitors changed, or if
    /// Hyprland lost it or kept a stale short one (the pointer can't reach those).
    fn watch(&self) {
        std::thread::spawn(|| {
            let sig = monitor_fingerprint();
            let layer = our_layer_height();
            util::on_main(move || with_dock(|d| d.apply_watch(sig, layer)));
        });
    }

    fn apply_watch(self: &Rc<Self>, sig: Option<String>, layer: Result<Option<i64>, ()>) {
        let (Some(sig), Ok(layer)) = (sig, layer) else {
            return;
        };
        let gtk_height = self
            .ui
            .borrow()
            .as_ref()
            .map_or(0, |ui| ui.window.allocated_height()) as i64;
        let rebuild = {
            let mut st = self.st.borrow_mut();
            if st
                .rebuilt_at
                .is_some_and(|t| t.elapsed() < Duration::from_secs(2))
            {
                if st.monitor_sig.is_none() {
                    st.monitor_sig = Some(sig);
                }
                return;
            }
            let changed = st.monitor_sig.as_ref().is_some_and(|old| *old != sig);
            let missing = layer.is_none();
            let short = layer.is_some_and(|h| gtk_height >= 40 && h > 0 && h < gtk_height - 15);
            if changed || missing || short {
                st.monitor_sig = Some(sig);
                true
            } else {
                false
            }
        };
        if rebuild {
            self.rebuild();
        }
    }

    // -- showing and hiding

    fn reveal(&self, on: bool) {
        if let Some(t) = self.hide_timer.borrow_mut().take() {
            t.remove();
        }
        self.st.borrow_mut().revealed = on;
        self.update();
    }

    fn schedule_hide(self: &Rc<Self>) {
        if let Some(t) = self.hide_timer.borrow_mut().take() {
            t.remove();
        }
        let me = Rc::downgrade(self);
        let id = glib::timeout_add_local_once(Duration::from_millis(350), move || {
            if let Some(d) = me.upgrade() {
                d.hide_timer.borrow_mut().take();
                d.st.borrow_mut().revealed = false;
                d.update();
            }
        });
        *self.hide_timer.borrow_mut() = Some(id);
    }

    fn pointer_inside(&self) -> bool {
        let ui = self.ui.borrow();
        let Some(ui) = ui.as_ref() else { return false };
        let Some(pointer) = gdk::Display::default()
            .and_then(|d| d.default_seat())
            .and_then(|s| s.pointer())
        else {
            return true;
        };
        let (_, x, y) = pointer.position();
        let (ox, oy) = ui.window.position();
        let (w, h) = ui.window.size();
        ox <= x && x < ox + w && oy <= y && y < oy + h
    }

    // -- actions

    fn activate(&self, app_id: &str) {
        let (open, held, active) = {
            let st = self.st.borrow();
            let all = st.windows.get(app_id).cloned().unwrap_or_default();
            let (held, mut open): (Vec<Client>, Vec<Client>) =
                all.into_iter().partition(|c| c.minimized);
            open.sort_by_key(|c| c.focus_history);
            (open, held, st.active.clone())
        };
        if open.is_empty() {
            match held.first() {
                Some(c) => self.restore(c),
                None => launch(app_id),
            }
            return;
        }
        let here = open.iter().any(|c| Some(&c.address) == active.as_ref());
        let i = {
            let mut st = self.st.borrow_mut();
            let next = if here && open.len() > 1 {
                (st.cycle.get(app_id).copied().unwrap_or(0) + 1) % open.len()
            } else {
                0
            };
            st.cycle.insert(app_id.to_string(), next);
            next
        };
        let address = open[i].address.clone();
        std::thread::spawn(move || {
            sys::hypr_dispatch(&format!(
                r#"hl.dsp.focus({{ window = "address:{address}" }})"#
            ));
        });
    }

    /// Bring a minimized window back to the workspace it came from.
    fn restore(&self, c: &Client) {
        let back = c.minimized_from().or(self.st.borrow().workspace_id);
        let Some(back) = back.filter(|b| *b >= 1) else {
            return;
        };
        let address = c.address.clone();
        std::thread::spawn(move || {
            let target = format!(r#"window = "address:{address}""#);
            sys::hypr_dispatch(&format!(
                r#"hl.dsp.window.move({{ workspace = "{back}", follow = false, {target} }})"#
            ));
            sys::hypr_dispatch(&format!(r#"hl.dsp.focus({{ {target} }})"#));
        });
    }

    fn quit_app(&self, app_id: &str) {
        let addresses: Vec<String> = self
            .st
            .borrow()
            .windows
            .get(app_id)
            .map(|cs| cs.iter().map(|c| c.address.clone()).collect())
            .unwrap_or_default();
        std::thread::spawn(move || {
            for a in addresses {
                sys::hypr_dispatch(&format!(
                    r#"hl.dsp.window.close({{ window = "address:{a}" }})"#
                ));
            }
        });
    }

    fn toggle_pin(self: &Rc<Self>, app_id: &str) {
        {
            let mut st = self.st.borrow_mut();
            if st.pins.iter().any(|p| p == app_id) {
                st.pins.retain(|p| p != app_id);
            } else {
                st.pins.push(app_id.to_string());
            }
            save_pins(&st.pins);
        }
        self.fill();
    }

    // -- menus

    fn popup(self: &Rc<Self>, menu: gtk::Menu, ev: &gdk::EventButton) {
        menu.style_context().add_class("np-dock");
        menu.show_all();
        if let Some(ui) = self.ui.borrow().as_ref() {
            menu.set_attach_widget(Some(&ui.row));
        }
        self.st.borrow_mut().menu_open = true;
        self.reveal(true);
        let me = Rc::downgrade(self);
        menu.connect_deactivate(move |_| {
            if let Some(d) = me.upgrade() {
                d.st.borrow_mut().menu_open = false;
                if d.pointer_inside() {
                    d.reveal(true); // still hovering: stay up
                } else {
                    d.schedule_hide();
                }
            }
        });
        menu.popup_at_pointer(Some(ev));
    }

    /// Right-click the grid button: the dock's own settings.
    fn dock_menu(self: &Rc<Self>, ev: &gdk::EventButton) {
        let menu = gtk::Menu::new();
        let item = gtk::CheckMenuItem::with_label("Auto-hide dock");
        item.set_active(self.st.borrow().autohide);
        let me = Rc::downgrade(self);
        item.connect_activate(move |it| {
            if let Some(d) = me.upgrade() {
                let on = it.is_active();
                d.st.borrow_mut().autohide = on;
                save_config(|m| {
                    m.insert("autohide".into(), Value::Bool(on));
                });
                d.update();
            }
        });
        menu.append(&item);
        self.popup(menu, ev);
    }

    fn app_menu(self: &Rc<Self>, app_id: &str, ev: &gdk::EventButton) {
        let menu = gtk::Menu::new();
        let installed = self.apps.borrow().all.contains_key(app_id);
        let pinned = self.st.borrow().pins.iter().any(|p| p == app_id);
        let open = self.st.borrow().windows.get(app_id).map_or(0, Vec::len);
        let add = |label: &str, action: Box<dyn Fn()>| {
            let item = gtk::MenuItem::with_label(label);
            item.connect_activate(move |_| action());
            menu.append(&item);
        };
        if installed {
            let id = app_id.to_string();
            add("New window", Box::new(move || launch(&id)));
        }
        let (me, id) = (Rc::downgrade(self), app_id.to_string());
        add(
            if pinned {
                "Unpin from dock"
            } else {
                "Pin to dock"
            },
            Box::new(move || {
                if let Some(d) = me.upgrade() {
                    d.toggle_pin(&id);
                }
            }),
        );
        if open > 0 {
            let (me, id) = (Rc::downgrade(self), app_id.to_string());
            let label = if open == 1 {
                "Quit".to_string()
            } else {
                format!("Quit all {open} windows")
            };
            add(
                &label,
                Box::new(move || {
                    if let Some(d) = me.upgrade() {
                        d.quit_app(&id);
                    }
                }),
            );
        }
        self.popup(menu, ev);
    }

    // -- signals from `dock pin`/`autohide` and `nameplate layout`

    fn reload(&self) {
        self.st.borrow_mut().pins = load_pins().unwrap_or_default();
        *self.apps.borrow_mut() = Apps::load();
        self.st.borrow_mut().state_sig = None;
        self.refresh();
    }

    fn reload_layout(&self) {
        self.st.borrow_mut().autohide = load_autohide();
        self.st.borrow_mut().state_sig = None;
        self.refresh();
    }
}

/// An app's icon at the normal and hover sizes, falling back to a generic one.
fn icon_for(app: Option<&gio::AppInfo>) -> (gtk::Image, Option<Pixbuf>, Option<Pixbuf>) {
    let theme = gtk::IconTheme::default();
    let load = |size: i32| -> Option<Pixbuf> {
        let theme = theme.as_ref()?;
        if let Some(icon) = app.and_then(|a| a.icon()) {
            if let Some(pix) = theme
                .lookup_by_gicon(&icon, size, gtk::IconLookupFlags::FORCE_SIZE)
                .and_then(|i| i.load_icon().ok())
            {
                return Some(pix);
            }
        }
        [
            "application-x-executable",
            "application-default-icon",
            "image-missing",
        ]
        .iter()
        .find_map(|name| {
            theme
                .load_icon(name, size, gtk::IconLookupFlags::FORCE_SIZE)
                .ok()
                .flatten()
        })
    };
    let (small, big) = (load(ICON), load(HOVER_ICON));
    let image = match &small {
        Some(pix) => gtk::Image::from_pixbuf(Some(pix)),
        None => gtk::Image::new(),
    };
    image.set_size_request(HOVER_ICON, HOVER_ICON); // growing on hover moves nothing
    image.set_valign(gtk::Align::Center);
    image.set_halign(gtk::Align::Center);
    (image, small, big)
}

/// Hyprland's event socket, on its own thread: refresh on window and workspace
/// changes, re-check the surface when monitors or the config change.
fn follow_events() {
    use std::io::BufRead;
    const WANTED: &[&str] = &[
        "openwindow",
        "closewindow",
        "movewindow",
        "workspace",
        "focusedmon",
        "activewindow",
        "fullscreen",
        "changefloatingmode",
        "monitoradded",
        "monitorremoved",
        "configreloaded",
    ];
    std::thread::spawn(|| loop {
        if let Some(events) = sys::hypr_events() {
            // Events from before this connection are lost: read the state now.
            util::on_main(|| with_dock(|d| d.refresh()));
            for line in events.lines() {
                let Ok(line) = line else { break };
                let name = line.split(">>").next().unwrap_or("");
                if ["monitoradded", "monitorremoved", "configreloaded"]
                    .iter()
                    .any(|w| name.starts_with(w))
                {
                    util::on_main(|| with_dock(|d| d.watch()));
                }
                if WANTED.iter().any(|w| name.starts_with(w)) {
                    util::on_main(|| with_dock(|d| d.schedule_refresh()));
                }
            }
        }
        std::thread::sleep(Duration::from_secs(2));
    });
}

pub fn run() -> Result<(), String> {
    if sys::state_dir().join("no-dock").exists() {
        return Ok(()); // turned off in Menu -> Appearance (bin/toggle dock)
    }
    let pidfile = sys::runtime_dir().join(PIDFILE);
    if !sys::claim_pidfile(&pidfile, MARKERS) {
        return Ok(());
    }
    let dock = Dock::new();
    DOCK.with(|d| *d.borrow_mut() = Some(dock.clone()));

    follow_events();
    glib::timeout_add_local(Duration::from_secs(3), || {
        with_dock(|d| d.watch());
        glib::ControlFlow::Continue
    });
    glib::unix_signal_add_local(ffi::SIGUSR1, || {
        with_dock(|d| d.reload());
        glib::ControlFlow::Continue
    });
    glib::unix_signal_add_local(ffi::SIGUSR2, || {
        with_dock(|d| d.reload_layout());
        glib::ControlFlow::Continue
    });
    glib::unix_signal_add_local(ffi::SIGTERM, || {
        gtk::main_quit();
        glib::ControlFlow::Break
    });
    // Newly installed or removed apps.
    let monitor = gio::AppInfoMonitor::get();
    monitor.connect_changed(|_| with_dock(|d| d.reload()));
    gtk::main();
    drop(monitor);
    Ok(())
}

// ------------------------------------------------------------ command line --

const USAGE: &str = "usage: nameplate-shell dock [pinned | pin APP | unpin APP | autohide [on|off]]
  APP is the app's .desktop name, e.g. firefox_firefox.desktop";

/// `nameplate-shell dock pinned | pin APP | unpin APP | autohide [on|off]`.
/// Needs no display.
pub fn cli(args: &[String]) -> Result<(), String> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let pins = pinned;
    match args.as_slice() {
        ["pinned"] => {
            for p in pins() {
                println!("{p}");
            }
        }
        ["autohide"] => println!("{}", if load_autohide() { "on" } else { "off" }),
        ["autohide", v @ ("on" | "off")] => {
            let on = *v == "on";
            save_config(|m| {
                m.insert("autohide".into(), Value::Bool(on));
            });
            poke(nameplate::ffi::SIGUSR2);
        }
        [cmd @ ("pin" | "unpin"), app] => {
            let app = if app.ends_with(".desktop") {
                app.to_string()
            } else {
                format!("{app}.desktop")
            };
            let mut list: Vec<String> = pins().into_iter().filter(|p| *p != app).collect();
            if *cmd == "pin" {
                list.push(app);
            }
            save_pins(&list);
            poke(nameplate::ffi::SIGUSR1);
        }
        _ => return Err(USAGE.into()),
    }
    Ok(())
}

/// The pinned apps' .desktop names, in dock order (the app grid's menu uses this).
pub fn pinned() -> Vec<String> {
    load_pins().unwrap_or_else(|| default_pins(&Apps::load()))
}

/// Pin an app to the dock or take it off.
pub fn set_pinned(app: &str, pin: bool) {
    let verb = if pin { "pin" } else { "unpin" };
    let _ = cli(&[verb.to_string(), app.to_string()]);
}

fn poke(signal: i32) {
    if let Some(pid) = sys::pidfile_owner(&sys::runtime_dir().join(PIDFILE), MARKERS) {
        nameplate::ffi::send_signal(pid, signal);
    }
}
