//! The wallpaper and theme pickers: Omarchy 4's slanted carousel, floating over
//! the desktop. The chosen one is big in the middle, the others tall slanted
//! slices on both sides, dimmed and overlapping; moving slides everything along.
//! Arrows (or h/l), scrolling or a click move; Enter, Space or a click on the big
//! one applies; Escape closes.

use super::{label, theme_colors, vbox, Anchor, Panel};
use gtk::gdk::prelude::GdkContextExt;
use gtk::gdk_pixbuf::{Colorspace, Pixbuf};
use gtk::prelude::*;
use gtk::{cairo, gdk, gio, glib};
use nameplate::sys;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

/// The chosen card.
const W: f64 = 768.0;
const H: f64 = 475.0;
/// The slices.
const SW: f64 = 108.0;
const SH: f64 = 432.0;
const SPACING: f64 = -30.0;
const SKEW: f64 = 28.0;
/// How many cards either side get their pictures loaded.
const RADIUS: usize = 8;

/// One card: a picture, and for themes the colours to paint the bar preview in.
pub struct Item {
    key: String,
    title: String,
    image: Option<PathBuf>,
    palette: Option<HashMap<String, String>>,
}

type Rgba = (f64, f64, f64, f64);
/// (item, x, y, w, h) of a card as drawn.
type Hit = (usize, f64, f64, f64, f64);

fn rgb(hex: &str, alpha: f64) -> Rgba {
    let h = hex.trim_start_matches('#');
    let part = |n: usize| h.get(n..n + 2).and_then(|p| u8::from_str_radix(p, 16).ok());
    match (part(0), part(2), part(4)) {
        (Some(r), Some(g), Some(b)) => (
            f64::from(r) / 255.0,
            f64::from(g) / 255.0,
            f64::from(b) / 255.0,
            alpha,
        ),
        _ => (0.5, 0.5, 0.5, alpha),
    }
}

fn source(cr: &cairo::Context, c: Rgba) {
    cr.set_source_rgba(c.0, c.1, c.2, c.3);
}

struct Carousel {
    area: gtk::DrawingArea,
    width: f64,
    items: Vec<Item>,
    sel: Cell<usize>,
    /// Where the strip is, between cards while it slides.
    pos: Cell<f64>,
    pix: RefCell<HashMap<String, Option<Pixbuf>>>,
    /// Loading passes: only the latest one stores pictures.
    gen: Cell<u64>,
    ticking: Cell<bool>,
    colors: HashMap<String, String>,
    /// (item, x, y, w, h) as last drawn, topmost last.
    hits: RefCell<Vec<Hit>>,
    on_select: Box<dyn Fn(usize)>,
    on_apply: Box<dyn Fn(usize)>,
}

impl Carousel {
    fn new(
        items: Vec<Item>,
        selected: usize,
        width: f64,
        on_select: impl Fn(usize) + 'static,
        on_apply: impl Fn(usize) + 'static,
    ) -> Rc<Self> {
        let area = gtk::DrawingArea::new();
        area.set_size_request(width as i32, (H + 8.0) as i32);
        area.add_events(
            gdk::EventMask::BUTTON_PRESS_MASK
                | gdk::EventMask::SCROLL_MASK
                | gdk::EventMask::SMOOTH_SCROLL_MASK,
        );
        let me = Rc::new(Carousel {
            area,
            width,
            items,
            sel: Cell::new(selected),
            pos: Cell::new(selected as f64),
            pix: RefCell::default(),
            gen: Cell::new(0),
            ticking: Cell::new(false),
            colors: theme_colors().into_iter().collect(),
            hits: RefCell::default(),
            on_select: Box::new(on_select),
            on_apply: Box::new(on_apply),
        });
        let weak = Rc::downgrade(&me);
        me.area.connect_draw(move |_, cr| {
            if let Some(me) = weak.upgrade() {
                me.draw(cr);
            }
            glib::Propagation::Proceed
        });
        let weak = Rc::downgrade(&me);
        me.area.connect_button_press_event(move |_, ev| {
            let Some(me) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let (x, y) = ev.position();
            let hit = me
                .hits
                .borrow()
                .iter()
                .rev()
                .find(|h| h.1 <= x && x <= h.1 + h.3 && h.2 <= y && y <= h.2 + h.4)
                .map(|h| h.0);
            match hit {
                Some(i) if i == me.sel.get() => (me.on_apply)(i),
                Some(i) => me.select(i as i64),
                None => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
        let weak = Rc::downgrade(&me);
        me.area.connect_scroll_event(move |_, ev| {
            if let Some(me) = weak.upgrade() {
                let step = match ev.direction() {
                    gdk::ScrollDirection::Down | gdk::ScrollDirection::Right => 1,
                    gdk::ScrollDirection::Up | gdk::ScrollDirection::Left => -1,
                    _ => match ev.scroll_deltas() {
                        Some((dx, dy)) if dx != 0.0 || dy != 0.0 => {
                            let d = if dx != 0.0 { dx } else { dy };
                            if d > 0.0 {
                                1
                            } else {
                                -1
                            }
                        }
                        _ => 0,
                    },
                };
                if step != 0 {
                    me.select(me.sel.get() as i64 + step);
                }
            }
            glib::Propagation::Stop
        });
        me.load_around();
        me
    }

    /// Decode the chosen picture first, then its neighbours, off the GTK thread
    /// (a 4K wallpaper takes a while), each as soon as it's ready.
    fn load_around(self: &Rc<Self>) {
        self.gen.set(self.gen.get() + 1);
        let gen = self.gen.get();
        let sel = self.sel.get();
        let mut order: Vec<usize> = (0..self.items.len()).collect();
        order.sort_by_key(|&i| i.abs_diff(sel));
        let have = self.pix.borrow();
        let want: Vec<(String, PathBuf)> = order
            .into_iter()
            .filter(|&i| i.abs_diff(sel) <= RADIUS && !have.contains_key(&self.items[i].key))
            .filter_map(|i| Some((self.items[i].key.clone(), self.items[i].image.clone()?)))
            .collect();
        drop(have);
        if want.is_empty() {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::MainContext::default().spawn_local(async move {
            for (key, path) in want {
                // Moved on: a newer pass owns loading now.
                if weak.upgrade().map_or(true, |me| me.gen.get() != gen) {
                    return;
                }
                if weak
                    .upgrade()
                    .is_some_and(|me| me.pix.borrow().contains_key(&key))
                {
                    continue;
                }
                // Pixels cross threads, not the image (which GTK owns).
                let raw =
                    gio::spawn_blocking(move || thumbnail(&path, W as i32, H as i32).map(Raw::of))
                        .await
                        .ok()
                        .flatten();
                let pix = raw.map(Raw::into_pixbuf);
                let Some(me) = weak.upgrade() else {
                    return;
                };
                // None too: don't retry a broken file forever.
                me.pix.borrow_mut().entry(key).or_insert(pix);
                me.area.queue_draw();
            }
        });
    }

    fn select(self: &Rc<Self>, i: i64) {
        let i = i.clamp(0, self.items.len() as i64 - 1) as usize;
        if i == self.sel.get() {
            return;
        }
        self.sel.set(i);
        (self.on_select)(i);
        self.load_around();
        if !self.ticking.replace(true) {
            let weak = Rc::downgrade(self);
            glib::timeout_add_local(Duration::from_millis(33), move || {
                let Some(me) = weak.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                let gap = me.sel.get() as f64 - me.pos.get();
                if gap.abs() < 0.01 {
                    me.pos.set(me.sel.get() as f64);
                    me.ticking.set(false);
                    me.area.queue_draw();
                    return glib::ControlFlow::Break;
                }
                me.pos.set(me.pos.get() + gap * 0.22); // ease out: quick, then settling
                me.area.queue_draw();
                glib::ControlFlow::Continue
            });
        }
        self.area.queue_draw();
    }

    /// Where a card sits at a (fractional) distance r from the middle.
    fn slot(&self, r: f64) -> (f64, f64, f64, f64) {
        let step = SW + SPACING;
        let px = (self.width - W) / 2.0;
        let far = |k: f64| {
            if k < 0.0 {
                (px + k * step, (H - SH) / 2.0, SW, SH)
            } else {
                (px + W + SPACING + (k - 1.0) * step, (H - SH) / 2.0, SW, SH)
            }
        };
        if r.abs() >= 1.0 {
            return far(r);
        }
        let (a, b, t) = (
            (px, 0.0, W, H),
            far(if r > 0.0 { 1.0 } else { -1.0 }),
            r.abs(),
        );
        (
            a.0 + (b.0 - a.0) * t,
            a.1 + (b.1 - a.1) * t,
            a.2 + (b.2 - a.2) * t,
            a.3 + (b.3 - a.3) * t,
        )
    }

    fn color(&self, key: &str, alpha: f64) -> Rgba {
        rgb(
            self.colors.get(key).map_or("#808080", String::as_str),
            alpha,
        )
    }

    fn draw(&self, cr: &cairo::Context) {
        let pos = self.pos.get();
        let mut visible: Vec<(usize, f64)> = (0..self.items.len())
            .map(|i| (i, i as f64 - pos))
            .filter(|(_, r)| r.abs() <= RADIUS as f64 + 1.0)
            .collect();
        // Far ones first, the chosen one on top.
        visible.sort_by(|a, b| b.1.abs().total_cmp(&a.1.abs()));
        let mut hits = Vec::new();
        let pix = self.pix.borrow();
        for (i, r) in visible {
            let (x, y, w, h) = self.slot(r);
            let y = y + 4.0;
            if x + w < 0.0 || x > self.width {
                continue;
            }
            let item = &self.items[i];
            let near = (1.0 - r.abs()).max(0.0); // 1 for the chosen one, 0 for a slice
            let _ = cr.save();
            outline(cr, x, y, w, h);
            cr.clip();
            source(cr, self.color("background", 1.0));
            let _ = cr.paint();
            let picture = pix.get(&item.key).cloned().flatten();
            if let Some(p) = &picture {
                let (pw, ph) = (f64::from(p.width()), f64::from(p.height()));
                let sc = (w / pw).max(h / ph);
                let _ = cr.save();
                cr.translate(x + (w - pw * sc) / 2.0, y + (h - ph * sc) / 2.0);
                cr.scale(sc, sc);
                cr.set_source_pixbuf(p, 0.0, 0.0);
                let _ = cr.paint();
                let _ = cr.restore();
            }
            if let Some(palette) = &item.palette {
                let _ = cr.save();
                cr.translate(x, y);
                paint_bar(cr, w, h, picture.is_some(), palette);
                let _ = cr.restore();
            }
            source(cr, self.color("background", 0.42 * (1.0 - near)));
            let _ = cr.paint();
            let _ = cr.restore();
            outline(cr, x, y, w, h);
            if near > 0.5 {
                source(cr, self.color("accent", 1.0));
                cr.set_line_width(3.0);
            } else {
                source(cr, self.color("muted", 1.0));
                cr.set_line_width(1.0);
            }
            let _ = cr.stroke();
            hits.push((i, x, y, w, h));
        }
        *self.hits.borrow_mut() = hits;
    }

    fn key(self: &Rc<Self>, event: &gdk::EventKey) -> bool {
        use gdk::keys::constants as key;
        let k = event.keyval();
        let sel = self.sel.get() as i64;
        if [key::Left, key::h, key::Up, key::ISO_Left_Tab].contains(&k) {
            self.select(sel - 1);
        } else if [key::Right, key::l, key::Down, key::Tab].contains(&k) {
            self.select(sel + 1);
        } else if k == key::Home {
            self.select(0);
        } else if k == key::End {
            self.select(self.items.len() as i64 - 1);
        } else if [key::Return, key::KP_Enter, key::space].contains(&k) {
            (self.on_apply)(self.sel.get());
        } else {
            return false;
        }
        true
    }
}

fn outline(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64) {
    cr.move_to(x + SKEW, y);
    cr.line_to(x + w, y);
    cr.line_to(x + w - SKEW, y + h);
    cr.line_to(x, y + h);
    cr.close_path();
}

// 6.2832 rather than TAU: the same arcs, to the pixel, as the Python version drew.
#[allow(clippy::approx_constant)]
/// A theme card: its wallpaper with the top bar over it in the theme's colours
/// (workspace numbers, the clock, the icons as dots). Side slices get nothing.
fn paint_bar(cr: &cairo::Context, w: f64, h: f64, has_picture: bool, c: &HashMap<String, String>) {
    let col = |key: &str, fallback: &str| rgb(c.get(key).map_or(fallback, String::as_str), 1.0);
    if !has_picture {
        source(cr, col("background", "#202020"));
        let _ = cr.paint();
    }
    if w < 300.0 {
        return;
    }
    let bar_h = (h * 0.052).max(9.0);
    source(cr, col("background", "#202020"));
    cr.rectangle(0.0, 0.0, w, bar_h);
    let _ = cr.fill();
    let keys = ["red", "yellow", "green", "cyan", "magenta", "blue"];
    for (k, key) in keys.iter().enumerate() {
        source(cr, col(key, "#808080"));
        cr.arc(
            w - w * 0.012 - (keys.len() - k) as f64 * bar_h * 0.62,
            bar_h / 2.0,
            bar_h * 0.13,
            0.0,
            6.2832,
        );
        let _ = cr.fill();
    }
    cr.select_font_face(
        "monospace",
        cairo::FontSlant::Normal,
        cairo::FontWeight::Normal,
    );
    cr.set_font_size(bar_h * 0.44);
    let base = bar_h * 0.68;
    let mut x = w * 0.012;
    for k in 0..5 {
        let num = (k + 1).to_string();
        source(
            cr,
            if k == 0 {
                col("foreground", "#808080")
            } else {
                col("dark_foreground", "#808080")
            },
        );
        cr.move_to(x, base);
        let _ = cr.show_text(&num);
        let nw = cr.text_extents(&num).map_or(0.0, |e| e.width());
        if k == 0 {
            // The workspace you're on.
            source(cr, col("accent", "#808080"));
            cr.set_line_width((bar_h * 0.07).max(1.0));
            cr.move_to(x, bar_h * 0.86);
            cr.line_to(x + nw, bar_h * 0.86);
            let _ = cr.stroke();
        }
        x += nw + bar_h * 0.5;
    }
    let clock = "Tue 20:25";
    source(cr, col("foreground", "#808080"));
    let cw = cr.text_extents(clock).map_or(0.0, |e| e.width());
    cr.move_to((w - cw) / 2.0, base);
    let _ = cr.show_text(clock);
}

/// A decoded picture's pixels, which may move between threads.
struct Raw {
    bytes: glib::Bytes,
    alpha: bool,
    width: i32,
    height: i32,
    stride: i32,
}

impl Raw {
    fn of(p: Pixbuf) -> Raw {
        Raw {
            bytes: p.read_pixel_bytes(),
            alpha: p.has_alpha(),
            width: p.width(),
            height: p.height(),
            stride: p.rowstride(),
        }
    }
    fn into_pixbuf(self) -> Pixbuf {
        Pixbuf::from_bytes(
            &self.bytes,
            Colorspace::Rgb,
            self.alpha,
            8,
            self.width,
            self.height,
            self.stride,
        )
    }
}

/// A cropped thumbnail, cached in ~/.cache/nameplate/thumbs.
fn thumbnail(path: &Path, w: i32, h: i32) -> Option<Pixbuf> {
    let mtime = std::fs::metadata(path).ok()?.modified().ok()?;
    let stamp = mtime
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let key = fnv1a(format!("{}:{stamp}:{w}x{h}", path.display()).as_bytes());
    let cache = sys::home().join(format!(".cache/nameplate/thumbs/{key:016x}.png"));
    if let Ok(p) = Pixbuf::from_file(&cache) {
        return Some(p);
    }
    let (_, iw, ih) = Pixbuf::file_info(path)?;
    if iw <= 0 || ih <= 0 {
        return None;
    }
    let s = (f64::from(w) / f64::from(iw)).max(f64::from(h) / f64::from(ih));
    let sw = w.max((f64::from(iw) * s).ceil() as i32);
    let sh = h.max((f64::from(ih) * s).ceil() as i32);
    let pix = Pixbuf::from_file_at_scale(path, sw, sh, false).ok()?;
    let crop = pix
        .new_subpixbuf((pix.width() - w) / 2, (pix.height() - h) / 2, w, h)
        .copy()?;
    if let Some(dir) = cache.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = crop.savev(&cache, "png", &[]);
    Some(crop)
}

/// A small stable hash for cache file names.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
    })
}

/// Width of the screen the panels open on (the one with the pointer).
fn screen_width() -> i32 {
    let Some(display) = gdk::Display::default() else {
        return 1366;
    };
    let monitor = display
        .default_seat()
        .and_then(|seat| seat.pointer())
        .map(|p| p.position())
        .and_then(|(_, x, y)| display.monitor_at_point(x, y))
        .or_else(|| display.monitor(0));
    monitor.map_or(1366, |m| m.geometry().width())
}

// ----------------------------------------------------------------- pickers --

/// What a picker does with the card you choose.
#[derive(Clone, Copy)]
enum Kind {
    Wallpapers,
    Themes,
}

pub struct Picker {
    root: gtk::Box,
    carousel: RefCell<Option<Rc<Carousel>>>,
}

impl Picker {
    fn build(kind: Kind, items: Vec<Item>, selected: usize, hint: &str) -> Rc<Self> {
        let width = (screen_width() - 80).min((W + 13.0 * (SW + SPACING) + 40.0) as i32);
        let root = vbox(14);
        root.set_size_request(width, -1);
        let me = Rc::new(Picker {
            root,
            carousel: RefCell::new(None),
        });
        if items.is_empty() {
            let l = label(hint, &["np-picker-name"]);
            l.set_xalign(0.5);
            me.root.pack_start(&l, false, false, 0);
            return me;
        }
        let name = label(&items[selected].title, &["np-picker-name"]);
        name.set_xalign(0.5);
        let titles: Vec<String> = items.iter().map(|i| i.title.clone()).collect();
        let keys: Vec<(String, Option<PathBuf>)> = items
            .iter()
            .map(|i| (i.key.clone(), i.image.clone()))
            .collect();
        let shown = name.downgrade();
        let carousel = Carousel::new(
            items,
            selected,
            f64::from(width),
            move |i| {
                if let Some(name) = shown.upgrade() {
                    name.set_text(&titles[i]);
                }
            },
            move |i| apply(kind, &keys[i].0),
        );
        me.root.pack_start(&carousel.area, false, false, 0);
        me.root.pack_start(&name, false, false, 0);
        let hint = label(hint, &["np-picker-hint"]);
        hint.set_xalign(0.5);
        me.root.pack_start(&hint, false, false, 0);
        *me.carousel.borrow_mut() = Some(carousel);
        me
    }

    pub fn wallpapers() -> Rc<Self> {
        let mut images: Vec<PathBuf> = crate::wallpaper::collection();
        let current = sys::read(sys::app_dir().join("wallpaper-current"))
            .filter(|s| !s.is_empty())
            .map_or_else(|| sys::app_dir().join("wallpaper.png"), PathBuf::from);
        if !images.contains(&current) && current.exists() {
            images.insert(0, current.clone());
        }
        let selected = images.iter().position(|p| *p == current).unwrap_or(0);
        let hint = if images.len() > 1 {
            "Enter or click to use it  ·  arrows or scroll to browse  ·  add your own in ~/Pictures/Wallpapers"
        } else {
            "Put images in ~/Pictures/Wallpapers, or get Omarchy's: ~/.config/nameplate/bin/get-omarchy-wallpapers"
        };
        let items = images
            .into_iter()
            .map(|p| Item {
                key: p.to_string_lossy().into_owned(),
                title: wallpaper_title(&p),
                image: Some(p),
                palette: None,
            })
            .collect();
        Self::build(Kind::Wallpapers, items, selected, hint)
    }

    pub fn themes() -> Rc<Self> {
        let listing = super::out(&sys::bin("theme").to_string_lossy(), &["list"]);
        let names: Vec<String> = listing
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.trim_start_matches(['*', ' ']).trim().to_string())
            .collect();
        let now = listing
            .lines()
            .find(|l| l.starts_with('*'))
            .map(|l| l.trim_start_matches(['*', ' ']).trim().to_string());
        let themes = sys::app_dir().join("themes");
        let walls = sys::home().join("Pictures/Wallpapers");
        // No theme wallpaper: your own current one stands behind the bar preview,
        // which is also what you'd get (theme set keeps it then).
        let current = sys::read(sys::app_dir().join("wallpaper-current"))
            .filter(|s| !s.is_empty())
            .map_or_else(|| sys::app_dir().join("wallpaper.png"), PathBuf::from);
        let items: Vec<Item> = names
            .iter()
            .map(|name| {
                let mut path = themes.join(format!("{name}.toml"));
                if !path.exists() {
                    path = sys::app_dir()
                        .join("user-themes")
                        .join(format!("{name}.toml"));
                }
                let image = theme_wallpaper(&walls, name)
                    .or_else(|| current.exists().then(|| current.clone()));
                Item {
                    key: name.clone(),
                    title: pretty(name),
                    image,
                    palette: Some(palette(&path)),
                }
            })
            .collect();
        let selected = now
            .and_then(|n| names.iter().position(|x| *x == n))
            .unwrap_or(0);
        let hint = if items.is_empty() {
            "No themes found. Run the installer again."
        } else {
            "Enter or click to switch  ·  arrows or scroll to browse"
        };
        Self::build(Kind::Themes, items, selected, hint)
    }
}

fn apply(kind: Kind, key: &str) {
    let key = key.to_string();
    match kind {
        Kind::Wallpapers => {
            // Its own thread: decoding for the lock screen can take a moment.
            std::thread::spawn(move || crate::wallpaper::cli(&["set".into(), key]));
        }
        Kind::Themes => {
            let theme = sys::bin("theme");
            sys::spawn(theme, &["set", &key]);
        }
    }
    super::close();
}

/// "3-Mountain_lake.jpg" -> "Mountain lake" (Omarchy numbers its wallpapers).
fn wallpaper_title(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let digits = stem.chars().take_while(char::is_ascii_digit).count();
    let rest = if digits > 0 {
        stem[digits..].trim_start_matches(['-', '_', ' '])
    } else {
        stem.as_str()
    };
    let name = if rest.is_empty() { stem.as_str() } else { rest };
    super::capitalize(name.replace(['-', '_'], " ").trim())
}

/// "tokyo-night" -> "Tokyo Night", with a few names spelled their own way.
fn pretty(name: &str) -> String {
    match name {
        "rose-pine" => "Rosé Pine".into(),
        "retro-82" => "Retro 82".into(),
        _ => {
            // Python's str.title(): a capital after anything that isn't a letter.
            let mut out = String::new();
            let mut after_letter = false;
            for ch in name.replace('-', " ").chars() {
                if after_letter {
                    out.extend(ch.to_lowercase());
                } else {
                    out.extend(ch.to_uppercase());
                }
                after_letter = ch.is_alphabetic();
            }
            out
        }
    }
}

/// The theme's own wallpaper, from Omarchy's set if you downloaded it.
fn theme_wallpaper(walls: &Path, name: &str) -> Option<PathBuf> {
    for folder in [walls.join("omarchy").join(name), walls.join(name)] {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        let mut images: Vec<String> = entries
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|f| {
                let low = f.to_lowercase();
                [".png", ".jpg", ".jpeg", ".webp"]
                    .iter()
                    .any(|x| low.ends_with(x))
            })
            .collect();
        images.sort();
        if let Some(first) = images.first() {
            return Some(folder.join(first));
        }
    }
    None
}

/// The theme's colours: `key = "#rrggbb"` lines.
fn palette(path: &Path) -> HashMap<String, String> {
    sys::read(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let (k, v) = line.split_once('=')?;
            let (k, v) = (k.trim(), v.trim());
            let hex = v.strip_prefix('"')?.get(..7)?;
            let ok = !k.is_empty()
                && k.chars().all(|c| c.is_alphanumeric() || c == '_')
                && hex.starts_with('#')
                && hex[1..].chars().all(|c| c.is_ascii_hexdigit())
                && v[1..].get(7..8) == Some("\"");
            ok.then(|| (k.to_string(), hex.to_string()))
        })
        .collect()
}

impl Panel for Picker {
    fn root(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }
    fn width(&self) -> i32 {
        self.root.size_request().0
    }
    fn anchor(&self) -> Anchor {
        Anchor::Middle
    }
    fn bare(&self) -> bool {
        true
    }
    fn on_key(self: Rc<Self>, event: &gdk::EventKey) -> bool {
        let carousel = self.carousel.borrow().clone();
        carousel.is_some_and(|c| c.key(event))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(
            wallpaper_title(Path::new("/w/3-Mountain_lake.jpg")),
            "Mountain lake"
        );
        assert_eq!(wallpaper_title(Path::new("/w/1999.png")), "1999");
        assert_eq!(
            wallpaper_title(Path::new("/w/kanagawa-WAVE.webp")),
            "Kanagawa wave"
        );
        assert_eq!(pretty("tokyo-night"), "Tokyo Night");
        assert_eq!(pretty("rose-pine"), "Rosé Pine");
        assert_eq!(pretty("catppuccin-latte"), "Catppuccin Latte");
        assert_eq!(rgb("#ff8000", 0.5), (1.0, 128.0 / 255.0, 0.0, 0.5));
        assert_eq!(rgb("zz", 1.0), (0.5, 0.5, 0.5, 1.0));
    }
}
