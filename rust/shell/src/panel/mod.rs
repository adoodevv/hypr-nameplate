//! The panels: small popups that open under the bar (Wi-Fi, sound, calendar...),
//! plus the full-screen ones (app grid, menus, pickers).
//!
//!   nameplate-shell panel KIND [ROUTE] [--bar]   open (or, when open, close) a panel
//!   nameplate-shell panel daemon                  keep running, started at login
//!   nameplate-shell panel reload | close          new theme or font / close what's open
//!
//! A click on the bar runs the short-lived client half, which hands the request
//! over a socket to the daemon; the daemon keeps GTK loaded so panels appear at
//! once. Only one panel is open at a time, and asking for the open one again
//! closes it.
//!
//! The panels are moving here from bin/panel (Python) one at a time. Kinds not
//! in [`KINDS`] are passed on to bin/panel's own daemon (its socket is
//! nameplate-panel-py.sock); before either side opens a panel it closes the
//! other's.

mod agents;
mod apps;
mod bluetooth;
mod calendar;
mod clipboard;
mod emoji;
mod media;
mod power;
mod sound;
mod weather;
mod wifi;

use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use gtk_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use nameplate::sys;
use std::cell::{Cell, RefCell};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::time::Duration;

/// The panels drawn here.
const KINDS: &[&str] = &[
    "clipboard",
    "apps",
    "calendar",
    "media",
    "power",
    "sound",
    "wifi",
    "bluetooth",
    "emoji",
    "weather",
    "agents",
];
/// Every panel, for the usage message and to refuse typos before starting anything.
const ALL: &[&str] = &[
    "calendar",
    "media",
    "sound",
    "wifi",
    "bluetooth",
    "display",
    "power",
    "wallpapers",
    "clipboard",
    "emoji",
    "menu",
    "themes",
    "weather",
    "agents",
    "apps",
];

fn sock() -> PathBuf {
    sys::runtime_dir().join("nameplate-panel.sock")
}

fn py_sock() -> PathBuf {
    sys::runtime_dir().join("nameplate-panel-py.sock")
}

/// Which panel is open ("kind route"), for the bar's accent line.
fn open_file() -> PathBuf {
    sys::runtime_dir().join("nameplate-panel-open")
}

fn send_to(path: &Path, message: &str) -> bool {
    UnixStream::connect(path)
        .and_then(|mut s| {
            s.set_write_timeout(Some(Duration::from_secs(1)))?;
            s.write_all(message.as_bytes())
        })
        .is_ok()
}

// ------------------------------------------------------------------ client --

pub fn cli(args: &[String]) -> Result<(), String> {
    let bar = args.iter().any(|a| a == "--bar");
    let args: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|a| *a != "--bar")
        .collect();
    match args.as_slice() {
        ["daemon"] => daemon(),
        [cmd @ ("reload" | "close")] => {
            send_to(&sock(), cmd); // nothing running: nothing to do
            Ok(())
        }
        [kind, rest @ ..] if ALL.contains(kind) => {
            // Where the click was, so the panel opens on that screen's bar.
            let x = if bar { cursor_x() } else { String::new() };
            let message = format!("open\t{kind}\t{}\t{x}", rest.first().unwrap_or(&""));
            if send_to(&sock(), &message) {
                return Ok(());
            }
            // No daemon yet (first click after an update, or it crashed): start one.
            if let Ok(me) = std::env::current_exe() {
                sys::spawn(me, &["panel", "daemon"]);
            }
            for _ in 0..60 {
                std::thread::sleep(Duration::from_millis(50));
                if send_to(&sock(), &message) {
                    return Ok(());
                }
            }
            Err("the panels didn't start".into())
        }
        _ => Err(format!(
            "usage: nameplate-shell panel {} [ROUTE] [--bar] | daemon | reload | close",
            ALL.join("|")
        )),
    }
}

fn cursor_x() -> String {
    sys::hypr("cursorpos")
        .and_then(|r| r.split(',').next().map(|x| x.trim().to_string()))
        .unwrap_or_default()
}

// ------------------------------------------------------------------ daemon --

fn daemon() -> Result<(), String> {
    if send_to(&sock(), "ping") {
        return Ok(()); // one is already running
    }
    gtk::init().map_err(|_| "no display")?;
    if !gtk_layer_shell::is_supported() {
        return Err("this desktop doesn't support layer-shell".into());
    }
    let path = sock();
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    install_css();
    write_open_file(None); // clears a line left by a crash
    start_python(); // so its panels open quickly too

    let fd = listener.as_raw_fd();
    glib::unix_fd_add_local(fd, glib::IOCondition::IN, move |_, _| {
        while let Ok((mut conn, _)) = listener.accept() {
            let _ = conn.set_nonblocking(false);
            let _ = conn.set_read_timeout(Some(Duration::from_secs(1)));
            let mut buf = Vec::new();
            let _ = (&mut conn).take(4096).read_to_end(&mut buf);
            handle(&String::from_utf8_lossy(&buf));
        }
        glib::ControlFlow::Continue
    });
    glib::unix_signal_add_local(nameplate::ffi::SIGTERM, move || {
        let _ = std::fs::remove_file(sock());
        gtk::main_quit();
        glib::ControlFlow::Break
    });
    gtk::main();
    Ok(())
}

fn handle(message: &str) {
    let parts: Vec<&str> = message.split('\t').collect();
    match parts.as_slice() {
        ["open", kind, rest @ ..] if KINDS.contains(kind) => {
            let route = rest.first().copied().filter(|r| !r.is_empty());
            let x = rest.get(1).and_then(|x| x.trim().parse::<f64>().ok());
            if current_kind().as_deref() == Some(*kind) {
                close(); // the same bar item again
            } else {
                open(kind, route, x.map(|x| x as i32));
            }
        }
        ["open", ..] => {
            if current_kind().is_some() {
                close_now();
            }
            relay(message.to_string());
        }
        ["reload"] => {
            install_css();
            relay(message.to_string());
        }
        ["close"] => {
            close();
            let _ = send_to(&py_sock(), "close");
        }
        // Tests only: click the button with this text or tooltip, in the open panel.
        ["press", text] if std::env::var_os("NAMEPLATE_PANEL_TEST").is_some() => press(text),
        _ => {}
    }
}

/// Pass a request to bin/panel, starting it if it isn't running.
fn relay(message: String) {
    std::thread::spawn(move || {
        if send_to(&py_sock(), &message) {
            return;
        }
        start_python();
        for _ in 0..60 {
            std::thread::sleep(Duration::from_millis(50));
            if send_to(&py_sock(), &message) {
                return;
            }
        }
    });
}

fn start_python() {
    let py = sys::bin("panel");
    if py.exists() {
        sys::spawn(py, &["daemon"]);
    }
}

/// Note which panel is open and wake the bar items that draw the accent line.
fn write_open_file(open: Option<(&str, Option<&str>)>) {
    let text = match open {
        Some((kind, route)) => format!("{kind} {}", route.unwrap_or("")).trim().to_string() + "\n",
        None => String::new(),
    };
    if sys::write_atomic(open_file(), &text).is_err() {
        return;
    }
    // Items still run once per signal: the panel items, the sound item, the weather.
    let rtmin = nameplate::ffi::sigrtmin();
    for p in sys::processes().iter().filter(|p| p.name == "waybar") {
        for n in [12, 9, 16] {
            nameplate::ffi::send_signal(p.pid, rtmin + n);
        }
    }
}

// ------------------------------------------------------------------- theme --

thread_local! {
    static STYLE: RefCell<Option<gtk::CssProvider>> = const { RefCell::new(None) };
}

/// The theme's colours (written by bin/theme), Tokyo Night where it says nothing.
fn theme_colors() -> Vec<(String, String)> {
    let mut colors: Vec<(String, String)> = [
        ("mode", "dark"),
        ("background", "#1a1b26"),
        ("selection", "#292e42"),
        ("muted", "#414868"),
        ("dark_foreground", "#565f89"),
        ("accent", "#7aa2f7"),
        ("foreground", "#a9b1d6"),
        ("bright_foreground", "#c0caf5"),
        ("red", "#f7768e"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    for line in sys::read(sys::app_dir().join("colors.toml"))
        .unwrap_or_default()
        .lines()
    {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let (k, v) = (k.trim(), v.trim());
        let ok_key = !k.is_empty() && k.chars().all(|c| c.is_alphanumeric() || c == '_');
        let Some(v) = v
            .strip_prefix('"')
            .and_then(|v| v.split_once('"'))
            .map(|p| p.0)
        else {
            continue;
        };
        if ok_key {
            match colors.iter_mut().find(|(name, _)| name == k) {
                Some(entry) => entry.1 = v.to_string(),
                None => colors.push((k.to_string(), v.to_string())),
            }
        }
    }
    colors
}

/// (Re)load the panels' look, after a theme or font change too.
fn install_css() {
    let colors = theme_colors();
    let color = |k: &str| {
        colors
            .iter()
            .find(|(name, _)| name == k)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };
    let mut css = String::new();
    for (name, key) in [
        ("bg", "background"),
        ("selection", "selection"),
        ("muted", "muted"),
        ("fg_dim", "dark_foreground"),
        ("accent", "accent"),
        ("fg", "foreground"),
        ("fg_bright", "bright_foreground"),
        ("red", "red"),
    ] {
        css.push_str(&format!("@define-color {name} {};\n", color(key)));
    }
    let font = sys::read(sys::app_dir().join("font"))
        .map(|f| f.trim().replace('"', ""))
        .filter(|f| !f.is_empty())
        .unwrap_or_else(|| "JetBrainsMono Nerd Font".into());
    css.push_str(
        &include_str!("panel.css").replace("\"JetBrainsMono Nerd Font\"", &format!("\"{font}\"")),
    );

    let Some(screen) = gdk::Screen::default() else {
        return;
    };
    let provider = gtk::CssProvider::new();
    if let Err(e) = provider.load_from_data(css.as_bytes()) {
        eprintln!("nameplate-shell panel: css: {e}");
    }
    STYLE.with(|s| {
        if let Some(old) = s.borrow_mut().replace(provider.clone()) {
            gtk::StyleContext::remove_provider_for_screen(&screen, &old);
        }
    });
    gtk::StyleContext::add_provider_for_screen(
        &screen,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_USER,
    );
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_application_prefer_dark_theme(color("mode") != "light");
    }
}

// ------------------------------------------------------------- the panels --

/// Where a panel sits.
#[derive(Clone, Copy, PartialEq)]
pub enum Anchor {
    /// Under the bar, at the right (the bar's panels).
    Right,
    /// Under the bar, in the middle (the calendar, under the clock).
    Top,
    /// In the middle of the screen (app grid, clipboard, menus).
    Middle,
}

/// One kind of panel. It builds its content into `root()`.
pub trait Panel {
    fn root(&self) -> gtk::Widget;
    fn width(&self) -> i32 {
        300
    }
    fn anchor(&self) -> Anchor {
        Anchor::Right
    }
    /// Dim the desktop behind it (the app grid, like Omarchy's).
    fn dim_backdrop(&self) -> bool {
        false
    }
    fn padding(&self) -> u32 {
        12
    }
    /// Called once the window is on screen (to focus a search line, say).
    fn shown(self: Rc<Self>) {}
    /// A key the window got first; true if the panel used it.
    fn on_key(&self, _event: &gdk::EventKey) -> bool {
        false
    }
}

struct Host {
    kind: String,
    windows: Vec<gtk::Window>,
    // Kept alive for as long as its windows are.
    _panel: Rc<dyn Panel>,
}

thread_local! {
    static CURRENT: RefCell<Option<Host>> = const { RefCell::new(None) };
    /// Bumped whenever a panel closes, so late results for it are dropped.
    static GENERATION: Cell<u64> = const { Cell::new(0) };
}

fn current_kind() -> Option<String> {
    CURRENT.with(|c| c.borrow().as_ref().map(|h| h.kind.clone()))
}

fn generation() -> u64 {
    GENERATION.with(Cell::get)
}

fn build(kind: &str, route: Option<&str>) -> Option<Rc<dyn Panel>> {
    Some(match kind {
        "clipboard" => clipboard::Clipboard::new(),
        "apps" => apps::Apps::new(),
        "calendar" => calendar::Calendar::new(route == Some("clock")),
        "media" => media::Media::new(false),
        "power" => power::Power::new(),
        "sound" => sound::Sound::new(),
        "wifi" => wifi::Wifi::new(),
        "bluetooth" => bluetooth::Bluetooth::new(),
        "emoji" => emoji::Emoji::new(),
        "weather" => weather::Weather::new(),
        "agents" => agents::Agents::new(),
        _ => return None,
    })
}

fn open(kind: &str, route: Option<&str>, mark_x: Option<i32>) {
    close_now();
    let _ = send_to(&py_sock(), "close");
    let Some(panel) = build(kind, route) else {
        return;
    };
    let monitor = mark_x.and_then(monitor_at); // the screen whose bar was clicked
    let mut windows = vec![backdrop(monitor.as_ref(), panel.dim_backdrop())];

    let win = gtk::Window::new(gtk::WindowType::Toplevel);
    win.style_context().add_class("np-popup");
    win.set_title(&format!("nameplate {kind}"));
    let frame = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    frame.set_border_width(panel.padding());
    let root = panel.root();
    root.set_size_request(panel.width(), -1);
    frame.add(&root);
    win.add(&frame);
    {
        let panel = panel.clone();
        win.connect_key_press_event(move |_, event| {
            if panel.on_key(event) {
                return glib::Propagation::Stop;
            }
            if event.keyval() == gdk::keys::constants::Escape {
                close();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
    }
    win.init_layer_shell();
    win.set_namespace("nameplate-panel");
    win.set_layer(Layer::Overlay);
    if let Some(m) = &monitor {
        win.set_monitor(m);
    }
    if panel.anchor() != Anchor::Middle {
        win.set_anchor(Edge::Top, true);
        win.set_layer_shell_margin(Edge::Top, 6);
    }
    if panel.anchor() == Anchor::Right {
        win.set_anchor(Edge::Right, true);
        win.set_layer_shell_margin(Edge::Right, 10);
    }
    win.set_keyboard_mode(KeyboardMode::Exclusive);
    windows.push(win.clone());
    for w in &windows {
        w.show_all();
    }
    win.set_focus(None::<&gtk::Widget>); // nothing starts focused (no focus ring)
    CURRENT.with(|c| {
        *c.borrow_mut() = Some(Host {
            kind: kind.to_string(),
            windows,
            _panel: panel.clone(),
        })
    });
    // Closed by the compositor or anything else: tidy up as if closed here.
    let gen = generation();
    win.connect_destroy(move |_| {
        if generation() == gen {
            close();
        }
    });
    write_open_file(Some((kind, route)));
    panel.shown();
}

fn press(text: &str) {
    fn find(w: &gtk::Widget, text: &str) -> Option<gtk::Button> {
        if let Some(b) = w.downcast_ref::<gtk::Button>() {
            let mut labels = Vec::new();
            collect_labels(b.upcast_ref(), &mut labels);
            if labels.iter().any(|l| l == text) || b.tooltip_text().is_some_and(|t| t == text) {
                return Some(b.clone());
            }
        }
        w.downcast_ref::<gtk::Container>()?
            .children()
            .iter()
            .find_map(|c| find(c, text))
    }
    fn collect_labels(w: &gtk::Widget, out: &mut Vec<String>) {
        if let Some(l) = w.downcast_ref::<gtk::Label>() {
            out.push(l.text().to_string());
        }
        if let Some(c) = w.downcast_ref::<gtk::Container>() {
            for child in c.children() {
                collect_labels(&child, out);
            }
        }
    }
    let windows = CURRENT.with(|c| c.borrow().as_ref().map(|h| h.windows.clone()));
    let button = windows
        .unwrap_or_default()
        .iter()
        .find_map(|w| find(w.upcast_ref(), text));
    match button {
        Some(b) => b.clicked(),
        None => eprintln!("nameplate-shell panel: no button {text:?}"),
    }
}

/// The screen at x (the pointer's position across all screens).
fn monitor_at(x: i32) -> Option<gdk::Monitor> {
    let display = gdk::Display::default()?;
    (0..display.n_monitors())
        .filter_map(|i| display.monitor(i))
        .find(|m| {
            let g = m.geometry();
            g.x() <= x && x < g.x() + g.width()
        })
}

/// A see-through layer over everything but the bar: a click on it closes the panel.
fn backdrop(monitor: Option<&gdk::Monitor>, dim: bool) -> gtk::Window {
    let w = gtk::Window::new(gtk::WindowType::Toplevel);
    w.style_context().add_class("np-backdrop");
    if dim {
        w.style_context().add_class("np-dimmed");
    }
    crate::util::transparent(&w);
    w.init_layer_shell();
    w.set_namespace("nameplate-backdrop");
    w.set_layer(Layer::Top);
    if let Some(m) = monitor {
        w.set_monitor(m);
    }
    for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        w.set_anchor(edge, true);
    }
    w.set_exclusive_zone(0); // clear of the bar, so its icons stay clickable
    w.set_keyboard_mode(KeyboardMode::None);
    w.add_events(gdk::EventMask::BUTTON_PRESS_MASK | gdk::EventMask::TOUCH_MASK);
    w.connect_button_press_event(|_, _| {
        close();
        glib::Propagation::Stop
    });
    w.connect_touch_event(|_, _| {
        close();
        glib::Propagation::Stop
    });
    w
}

/// Close the open panel. The bar's accent line goes a moment later, unless
/// another panel has opened by then.
pub fn close() {
    if close_now() {
        glib::idle_add_local_once(|| {
            if current_kind().is_none() {
                // Only clear what this side wrote: bin/panel may have one open now.
                let (kind, _) = sys::open_panel();
                if kind.is_empty() || KINDS.contains(&kind.as_str()) {
                    write_open_file(None);
                }
            }
        });
    }
}

fn close_now() -> bool {
    let Some(host) = CURRENT.with(|c| c.borrow_mut().take()) else {
        return false;
    };
    GENERATION.with(|g| g.set(g.get() + 1));
    for w in host.windows {
        // SAFETY: these windows are ours alone and not used after this.
        unsafe { w.destroy() };
    }
    true
}

// ----------------------------------------------------------------- helpers --

/// Run `work` on a worker thread, then `done` with its result on the GTK
/// thread, unless the panel that asked has closed in the meantime.
pub fn background<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
    done: impl FnOnce(T) + 'static,
) {
    let gen = generation();
    glib::MainContext::default().spawn_local(async move {
        if let Ok(result) = gio::spawn_blocking(work).await {
            if generation() == gen {
                done(result);
            }
        }
    });
}

/// How a command went.
pub enum Ran {
    Ok(Vec<u8>),
    /// Its output and errors, for a panel to show.
    Failed(Vec<u8>),
    Missing,
    TimedOut,
}

/// Run a command with optional input, giving up after `timeout`.
pub fn run(program: &str, args: &[&str], input: Option<&[u8]>, timeout: Duration) -> Ran {
    let child = Command::new(program)
        .args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ran::Missing,
        Err(_) => return Ran::Failed(Vec::new()),
    };
    if let (Some(data), Some(mut stdin)) = (input, child.stdin.take()) {
        let data = data.to_vec();
        std::thread::spawn(move || stdin.write_all(&data));
    }
    let pid = child.id();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    match rx.recv_timeout(timeout) {
        Ok(Ok(out)) if out.status.success() => Ran::Ok(out.stdout),
        Ok(Ok(out)) => Ran::Failed([out.stdout, out.stderr].concat()),
        Ok(Err(_)) => Ran::Failed(Vec::new()),
        Err(_) => {
            nameplate::ffi::send_signal(pid, nameplate::ffi::SIGKILL);
            Ran::TimedOut
        }
    }
}

pub fn label(text: &str, classes: &[&str]) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    for c in classes {
        l.style_context().add_class(c);
    }
    l
}

pub fn hbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Horizontal, spacing)
}

pub fn vbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Vertical, spacing)
}

/// A full-width clickable row: icon, text, and optional right-hand text.
pub fn row(icon: &str, text: &str, right: &str, on_click: impl Fn() + 'static) -> gtk::Button {
    let b = gtk::Button::new();
    let bx = hbox(10);
    bx.pack_start(&label(icon, &["np-icon"]), false, false, 0);
    let t = label(text, &[]);
    t.set_ellipsize(gtk::pango::EllipsizeMode::End);
    t.set_max_width_chars(26);
    t.set_width_chars(10);
    bx.pack_start(&t, true, true, 0);
    if !right.is_empty() {
        bx.pack_end(&label(right, &["np-dim"]), false, false, 0);
    }
    b.add(&bx);
    b.connect_clicked(move |_| on_click());
    b
}

/// A round icon button (media controls).
pub fn icon_button(
    glyph: &str,
    classes: &[&str],
    tooltip: &str,
    on_click: impl Fn() + 'static,
) -> gtk::Button {
    let b = gtk::Button::with_label(glyph);
    b.style_context().add_class("np-round");
    for c in classes {
        b.style_context().add_class(c);
    }
    if !tooltip.is_empty() {
        b.set_tooltip_text(Some(tooltip));
    }
    b.connect_clicked(move |_| on_click());
    b
}

pub fn separator() -> gtk::Separator {
    gtk::Separator::new(gtk::Orientation::Horizontal)
}

/// Every `ms` milliseconds while `target` (a panel) is alive, call `tick` with it.
pub fn every<T: 'static>(ms: u64, target: &Rc<T>, tick: impl Fn(&Rc<T>) + 'static) {
    let weak = Rc::downgrade(target);
    glib::timeout_add_local(Duration::from_millis(ms), move || match weak.upgrade() {
        Some(t) => {
            tick(&t);
            glib::ControlFlow::Continue
        }
        None => glib::ControlFlow::Break,
    });
}

/// Once, after `ms` milliseconds, if `target` is still alive.
pub fn later<T: 'static>(ms: u64, target: &Rc<T>, then: impl FnOnce(&Rc<T>) + 'static) {
    let weak = Rc::downgrade(target);
    glib::timeout_add_local_once(Duration::from_millis(ms), move || {
        if let Some(t) = weak.upgrade() {
            then(&t);
        }
    });
}

/// Python's str.capitalize: "spotify" -> "Spotify", "VLC" -> "Vlc".
pub fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first
            .to_uppercase()
            .chain(chars.flat_map(char::to_lowercase))
            .collect(),
        None => String::new(),
    }
}

/// Whether a command succeeded, and everything it printed (or why it couldn't
/// run), trimmed: what a panel shows when something fails.
pub fn run_text(program: &str, args: &[&str], timeout: Duration) -> (bool, String) {
    match run(program, args, None, timeout) {
        Ran::Ok(o) => (true, String::from_utf8_lossy(&o).trim().to_string()),
        Ran::Failed(o) => (false, String::from_utf8_lossy(&o).trim().to_string()),
        Ran::Missing => (false, format!("{program} is not installed")),
        Ran::TimedOut => (false, "timed out".into()),
    }
}

/// Text output of a command that succeeded, empty otherwise.
pub fn out(program: &str, args: &[&str]) -> String {
    match run(program, args, None, Duration::from_secs(5)) {
        Ran::Ok(o) => String::from_utf8_lossy(&o).trim().to_string(),
        _ => String::new(),
    }
}

pub fn clear(container: &gtk::Box) {
    for child in container.children() {
        container.remove(&child);
    }
}

pub fn notify(title: &str, body: &str) {
    sys::spawn("notify-send", &["-a", title, "-t", "1500", body]);
}

/// Nerd Font icons the panels use.
pub mod icon {
    pub const CHECK: &str = "\u{f012c}";
    /// Wi-Fi signal from none to full.
    pub const WIFI: [&str; 5] = [
        "\u{f092f}",
        "\u{f091f}",
        "\u{f0922}",
        "\u{f0925}",
        "\u{f0928}",
    ];
    pub const LOCK: &str = "\u{f033e}";
    pub const BLUETOOTH: &str = "\u{f00af}";
    pub const BLUETOOTH_CONNECTED: &str = "\u{f00b1}";
    pub const PLUS: &str = "\u{f0415}";
    pub const UNLINK: &str = "\u{f0338}";
    /// Volume low, medium, high.
    pub const VOLUME: [&str; 3] = ["\u{f057f}", "\u{f0580}", "\u{f057e}"];
    pub const MUTE: &str = "\u{f0581}";
    pub const SPEAKER: &str = "\u{f04c3}";
    pub const HEADPHONES: &str = "\u{f02cb}";
    pub const MIXER: &str = "\u{f066a}";
    pub const REFRESH: &str = "\u{f0450}";
    pub const COG: &str = "\u{f0493}";
    pub const PREV: &str = "\u{f04ae}";
    pub const NEXT: &str = "\u{f04ad}";
    pub const PLAY: &str = "\u{f040a}";
    pub const PAUSE: &str = "\u{f03e4}";
    pub const MUSIC: &str = "\u{f075a}";
    pub const CHARGING: &str = "\u{f0084}";
    pub const LEAF: &str = "\u{f032a}";
    pub const BALANCE: &str = "\u{f05d1}";
    pub const ROCKET: &str = "\u{f0463}";
    /// Battery from empty to full, in tenths.
    pub const BATTERY: [&str; 11] = [
        "\u{f008e}",
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
    pub const CLIP: &str = "\u{f0147}";
    pub const IMAGE: &str = "\u{f02e9}";
    pub const TRASH: &str = "\u{f0a7a}";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_passes_input_and_times_out() {
        let t = Duration::from_secs(5);
        assert!(
            matches!(run("cat", &[], Some(b"a\tb\xff\n"), t), Ran::Ok(o) if o == b"a\tb\xff\n")
        );
        assert!(matches!(
            run("sh", &["-c", "cat >/dev/null; exit 3"], Some(b"x"), t),
            Ran::Failed(_)
        ));
        assert!(matches!(
            run("no-such-program-np", &[], None, t),
            Ran::Missing
        ));
        assert!(matches!(
            run("sleep", &["5"], None, Duration::from_millis(200)),
            Ran::TimedOut
        ));
    }
}
