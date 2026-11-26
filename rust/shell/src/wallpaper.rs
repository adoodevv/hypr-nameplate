//! The desktop wallpaper: one background-layer window per screen, the image
//! scaled to cover it, a slanted wipe from the centre when it changes (Omarchy's
//! 840 ms reveal), and double-click to open the pickers (left: wallpapers,
//! right: themes).
//!
//! The command line (`set`, `next`, `list`, `lock`) records the choice in
//! ~/.config/nameplate/wallpaper-current and sends the running wallpaper
//! SIGUSR1 through its pidfile.
//!
//! The collection is the default wallpaper plus every image under
//! ~/Pictures/Wallpapers, sub-folders included, in name order.

use crate::util;
use gtk::prelude::*;
use gtk::{gdk, gdk_pixbuf::Pixbuf, glib};
use gtk_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use nameplate::sys;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

const WIPE: Duration = Duration::from_millis(840);
const SLANT: f64 = -0.18;
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

const PIDFILE: &str = "nameplate-wallpaper.pid";
const MARKERS: &[&str] = &["nameplate-shell wallpaper"];
const EXTS: &[&str] = &["png", "jpg", "jpeg", "webp"];

fn current() -> PathBuf {
    let default = sys::app_dir().join("wallpaper.png");
    sys::read(sys::app_dir().join("wallpaper-current"))
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .unwrap_or(default)
}

/// `path` scaled to cover `w` x `h` device pixels, cropped to the centre.
fn cover(path: &std::path::Path, w: i32, h: i32) -> Option<Pixbuf> {
    let (_, iw, ih) = Pixbuf::file_info(path)?;
    if iw <= 0 || ih <= 0 || w <= 0 || h <= 0 {
        return None;
    }
    let s = (w as f64 / iw as f64).max(h as f64 / ih as f64);
    let sw = w.max((iw as f64 * s).ceil() as i32);
    let sh = h.max((ih as f64 * s).ceil() as i32);
    let pix = Pixbuf::from_file_at_scale(path, sw, sh, false).ok()?;
    let x = (pix.width() - w) / 2;
    let y = (pix.height() - h) / 2;
    Some(pix.new_subpixbuf(x, y, w, h))
}

#[derive(Default)]
struct State {
    size: Option<(i32, i32, i32)>, // logical width, height, scale factor
    path: Option<PathBuf>,
    shown: Option<Pixbuf>,
    incoming: Option<Pixbuf>,
    progress: f64,
    started: Option<Instant>,
    animating: bool,
    last_press: Option<(u32, Instant, f64, f64)>,
    handled: Option<(u32, Instant)>,
}

struct Surface {
    window: gtk::Window,
    area: gtk::DrawingArea,
    state: RefCell<State>,
}

impl Surface {
    fn new(monitor: &gdk::Monitor) -> Rc<Surface> {
        let window = gtk::Window::new(gtk::WindowType::Toplevel);
        window.style_context().add_class("np-wallpaper");
        window.init_layer_shell();
        window.set_namespace("nameplate-wallpaper");
        window.set_layer(Layer::Background);
        window.set_monitor(monitor);
        for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            window.set_anchor(edge, true);
        }
        window.set_exclusive_zone(-1); // behind the bar too
        window.set_keyboard_mode(KeyboardMode::None);

        let events = gtk::EventBox::new();
        let area = gtk::DrawingArea::new();
        events.add(&area);
        window.add(&events);

        let surface = Rc::new(Surface {
            window,
            area,
            state: RefCell::new(State {
                progress: 1.0,
                ..State::default()
            }),
        });

        let s = Rc::downgrade(&surface);
        surface.area.connect_draw(move |_, cr| {
            if let Some(s) = s.upgrade() {
                s.paint(cr);
            }
            glib::Propagation::Stop
        });
        let s = Rc::downgrade(&surface);
        events.connect_size_allocate(move |w, alloc| {
            if let Some(s) = s.upgrade() {
                let size = (alloc.width(), alloc.height(), w.scale_factor());
                let changed = s.state.borrow().size != Some(size);
                if changed {
                    {
                        let mut st = s.state.borrow_mut();
                        st.size = Some(size);
                        st.shown = None;
                        st.incoming = None;
                        st.progress = 1.0;
                    }
                    let s2 = Rc::downgrade(&s);
                    glib::idle_add_local_once(move || {
                        if let Some(s) = s2.upgrade() {
                            s.load(false);
                        }
                    });
                }
            }
        });
        let s = Rc::downgrade(&surface);
        events.connect_button_press_event(move |_, ev| {
            if let Some(s) = s.upgrade() {
                s.clicked(ev);
            }
            glib::Propagation::Stop
        });
        surface.window.show_all();
        surface
    }

    /// Show the current wallpaper: wipe in a new one, or swap at once (`force`,
    /// or nothing on screen yet).
    fn load(self: &Rc<Self>, force: bool) {
        let path = current();
        let Some((w, h, sf)) = self.state.borrow().size else {
            return;
        };
        {
            let st = self.state.borrow();
            if !force
                && st.path.as_ref() == Some(&path)
                && st.shown.is_some()
                && st.incoming.is_none()
            {
                return;
            }
        }
        let new = cover(&path, w * sf, h * sf);
        let mut st = self.state.borrow_mut();
        st.path = Some(path);
        let Some(new) = new else {
            st.shown = None;
            st.incoming = None;
            drop(st);
            self.area.queue_draw();
            return;
        };
        if st.shown.is_none() || force {
            st.shown = Some(new);
            st.incoming = None;
            st.progress = 1.0;
            drop(st);
            self.area.queue_draw();
            return;
        }
        // A change during a wipe lands the image that was arriving first.
        if let Some(arriving) = st.incoming.take() {
            st.shown = Some(arriving);
        }
        st.incoming = Some(new);
        st.progress = 0.0;
        st.started = Some(Instant::now());
        let start_timer = !st.animating;
        st.animating = true;
        drop(st);
        if start_timer {
            let s = Rc::downgrade(self);
            glib::timeout_add_local(Duration::from_millis(16), move || match s.upgrade() {
                Some(s) => s.step(),
                None => glib::ControlFlow::Break,
            });
        }
        self.area.queue_draw();
    }

    fn step(&self) -> glib::ControlFlow {
        let mut st = self.state.borrow_mut();
        let t = st.started.map_or(1.0, |s| {
            (s.elapsed().as_secs_f64() / WIPE.as_secs_f64()).min(1.0)
        });
        st.progress = 1.0 - (1.0 - t).powi(3); // ease-out cubic
        let done = t >= 1.0;
        if done {
            if let Some(arrived) = st.incoming.take() {
                st.shown = Some(arrived);
            }
            st.animating = false;
        }
        drop(st);
        self.area.queue_draw();
        if done {
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    }

    fn paint(&self, cr: &gtk::cairo::Context) {
        let st = self.state.borrow();
        let Some((w, h, sf)) = st.size else { return };
        let (w, h) = (w as f64, h as f64);
        cr.save().ok();
        cr.scale(1.0 / sf as f64, 1.0 / sf as f64);
        if let Some(pix) = &st.shown {
            cr.set_source_pixbuf(pix, 0.0, 0.0);
            let _ = cr.paint();
        }
        if let Some(pix) = &st.incoming {
            if st.progress < 1.0 {
                // The slanted band growing out from the centre.
                let (w, h, sfd) = (w * sf as f64, h * sf as f64, sf as f64);
                let top = w / 2.0 - SLANT * h / 2.0;
                let bottom = w / 2.0 + SLANT * h / 2.0;
                let spread = (w / 2.0 + SLANT.abs() * h / 2.0 + 4.0 * sfd) * st.progress;
                cr.move_to(top - spread, 0.0);
                cr.line_to(top + spread, 0.0);
                cr.line_to(bottom + spread, h);
                cr.line_to(bottom - spread, h);
                cr.close_path();
                cr.clip();
            }
            cr.set_source_pixbuf(pix, 0.0, 0.0);
            let _ = cr.paint();
        }
        cr.restore().ok();
    }

    fn clicked(&self, ev: &gdk::EventButton) {
        let button = ev.button();
        let (x, y) = ev.position();
        let mut st = self.state.borrow_mut();
        if ev.event_type() == gdk::EventType::DoubleButtonPress {
            let already = st
                .handled
                .is_some_and(|(b, t)| b == button && t.elapsed() < Duration::from_millis(600));
            st.last_press = None;
            st.handled = None;
            drop(st);
            if !already {
                open_picker(button);
            }
            return;
        }
        if ev.event_type() != gdk::EventType::ButtonPress || !(button == 1 || button == 3) {
            return;
        }
        // Right double-clicks don't reliably arrive as 2BUTTON_PRESS, so spot
        // the second press here as well.
        let now = Instant::now();
        let second = st.last_press.is_some_and(|(b, t, px, py)| {
            b == button
                && t.elapsed() < DOUBLE_CLICK
                && (x - px).abs() < 32.0
                && (y - py).abs() < 32.0
        });
        if second {
            st.last_press = None;
            st.handled = Some((button, now));
            drop(st);
            open_picker(button);
        } else {
            st.last_press = Some((button, now, x, y));
        }
    }
}

fn open_picker(button: u32) {
    let shell = sys::bin("nameplate-shell");
    match button {
        1 => sys::spawn(&shell, &["panel", "wallpapers"]),
        3 => sys::spawn(&shell, &["panel", "themes"]),
        _ => {}
    }
}

pub fn run() -> Result<(), String> {
    let pidfile = sys::runtime_dir().join(PIDFILE);
    if !sys::claim_pidfile(&pidfile, MARKERS) {
        return Ok(());
    }
    // The lock screen copy follows the wallpaper; make sure it exists after a
    // fresh install or a change made while this wasn't running.
    refresh_lock_in_background();

    util::add_css(&format!(
        "window.np-wallpaper {{ background-color: {}; }}",
        sys::theme_color("background")
    ));

    let surfaces: Rc<RefCell<Vec<Rc<Surface>>>> = Rc::default();
    let rebuild = {
        let surfaces = surfaces.clone();
        move || {
            for s in surfaces.borrow_mut().drain(..) {
                unsafe { s.window.destroy() };
            }
            let display = gdk::Display::default().expect("a display");
            for i in 0..display.n_monitors() {
                if let Some(m) = display.monitor(i) {
                    surfaces.borrow_mut().push(Surface::new(&m));
                }
            }
        }
    };
    rebuild();

    let display = gdk::Display::default().ok_or("no display")?;
    for signal in ["monitor-added", "monitor-removed"] {
        let rebuild = rebuild.clone();
        display.connect_local(signal, false, move |_| {
            let rebuild = rebuild.clone();
            glib::timeout_add_local_once(Duration::from_millis(500), rebuild);
            None
        });
    }
    {
        let surfaces = surfaces.clone();
        glib::unix_signal_add_local(nameplate::ffi::SIGUSR1, move || {
            let now = current();
            for s in surfaces.borrow().iter() {
                let same = s.state.borrow().path.as_ref() == Some(&now);
                s.load(same); // same path: new contents, swap at once; else wipe
            }
            glib::ControlFlow::Continue
        });
    }
    glib::unix_signal_add_local(nameplate::ffi::SIGTERM, || {
        gtk::main_quit();
        glib::ControlFlow::Break
    });
    gtk::main();
    Ok(())
}

// ------------------------------------------------------------ command line --

/// `nameplate-shell wallpaper set IMAGE | next | list | lock`. Needs no display.
pub fn cli(args: &[String]) -> Result<(), String> {
    match args {
        [cmd, image] if cmd == "set" => set(Path::new(image)),
        [cmd] if cmd == "next" => {
            let images = collection();
            if images.is_empty() {
                return Ok(());
            }
            let now = current();
            let next = match images.iter().position(|p| *p == now) {
                Some(i) => (i + 1) % images.len(),
                None => 0,
            };
            set(&images[next])
        }
        [cmd] if cmd == "list" => {
            for p in collection() {
                println!("{}", p.display());
            }
            Ok(())
        }
        [cmd] if cmd == "lock" => lock_image(),
        _ => Err("usage: nameplate-shell wallpaper [set IMAGE | next | list | lock]".into()),
    }
}

/// The default wallpaper and everything in ~/Pictures/Wallpapers (the picker shows these).
pub fn collection() -> Vec<PathBuf> {
    let mut images = Vec::new();
    let default = sys::app_dir().join("wallpaper.png");
    if default.exists() {
        images.push(default);
    }
    walk(
        &sys::home().join("Pictures").join("Wallpapers"),
        &mut images,
    );
    images
}

/// Images in `dir` in name order, then each sub-folder's (symlinked folders
/// are listed but not entered, so a loop can't trap it).
fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let (mut dirs, mut files) = (Vec::new(), Vec::new());
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if !entry.file_type().is_ok_and(|t| t.is_symlink()) {
                dirs.push(path);
            }
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| EXTS.contains(&e.to_ascii_lowercase().as_str()))
        {
            files.push(path);
        }
    }
    files.sort();
    dirs.sort();
    out.extend(files);
    for d in dirs {
        walk(&d, out);
    }
}

fn set(image: &Path) -> Result<(), String> {
    let path = if image.is_absolute() {
        image.to_path_buf()
    } else if let Ok(rest) = image.strip_prefix("~") {
        sys::home().join(rest)
    } else {
        std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(image)
    };
    if !path.exists() {
        return Err(format!("no such image: {}", path.display()));
    }
    sys::write_atomic(
        sys::app_dir().join("wallpaper-current"),
        &format!("{}\n", path.display()),
    )
    .map_err(|e| e.to_string())?;
    if let Some(pid) = sys::pidfile_owner(&sys::runtime_dir().join(PIDFILE), MARKERS) {
        nameplate::ffi::send_signal(pid, nameplate::ffi::SIGUSR1);
    }
    // The lock screen copy follows in the background: the desktop switches at
    // once instead of waiting on a 4K decode and a PNG save.
    refresh_lock_in_background();
    Ok(())
}

fn refresh_lock_in_background() {
    let lock_src = sys::read(sys::app_dir().join("lock-source")).map(PathBuf::from);
    if lock_src.as_ref() == Some(&current()) && sys::app_dir().join("lock.png").exists() {
        return;
    }
    if let Ok(me) = std::env::current_exe() {
        sys::spawn(me, &["wallpaper", "lock"]);
    }
}

/// Save the current wallpaper as a PNG for hyprlock, which reads one fixed path
/// (~/.config/nameplate/lock.png) and blurs it itself.
fn lock_image() -> Result<(), String> {
    let path = current();
    let mut pix = Pixbuf::from_file(&path).map_err(|e| format!("lock image: {e}"))?;
    let (w, h) = (pix.width(), pix.height());
    if w > 2560 {
        // Plenty for a blurred background, and quick for hyprlock to load.
        let scaled_h = (f64::from(h) * 2560.0 / f64::from(w)).round() as i32;
        pix = pix
            .scale_simple(2560, scaled_h, gtk::gdk_pixbuf::InterpType::Bilinear)
            .ok_or("lock image: out of memory")?;
    }
    let dir = sys::app_dir();
    let tmp = dir.join("lock.png.tmp");
    pix.savev(&tmp, "png", &[])
        .map_err(|e| format!("lock image: {e}"))?;
    std::fs::rename(&tmp, dir.join("lock.png")).map_err(|e| e.to_string())?;
    sys::write_atomic(dir.join("lock-source"), &format!("{}\n", path.display()))
        .map_err(|e| e.to_string())
}
