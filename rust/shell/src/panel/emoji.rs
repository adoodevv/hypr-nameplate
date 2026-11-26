//! Emoji picker (SUPER + CTRL + E): search by name or everyday word, click one
//! (or press Enter for the first) to copy it. Recently picked ones come first.

use super::{label, notify, run, vbox, Anchor, Panel, Ran};
use gtk::prelude::*;
use nameplate::sys;
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

/// How many to draw at once (the rest are a search away).
const SHOW: usize = 280;
const RECENT_KEPT: usize = 20;

pub struct Emoji {
    root: gtk::Box,
    search: gtk::SearchEntry,
    flow: gtk::FlowBox,
    hint: gtk::Label,
    /// (emoji, words), in the picker's order.
    all: Vec<(String, String)>,
    recent: Vec<String>,
    matches: RefCell<Vec<String>>,
}

fn recent_file() -> PathBuf {
    sys::home().join(".cache/nameplate/emoji-recent")
}

impl Emoji {
    pub fn new() -> Rc<Self> {
        let root = vbox(6);
        root.pack_start(&label("Emoji", &["np-title"]), false, false, 0);
        let search = gtk::SearchEntry::new();
        search.style_context().add_class("np-search");
        search.set_placeholder_text(Some("Search, e.g. heart, fire, laugh"));
        root.pack_start(&search, false, false, 0);
        let scroll = gtk::ScrolledWindow::new(gtk::Adjustment::NONE, gtk::Adjustment::NONE);
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroll.set_min_content_height(320);
        let flow = gtk::FlowBox::new();
        flow.set_selection_mode(gtk::SelectionMode::None);
        flow.set_max_children_per_line(10);
        flow.set_min_children_per_line(10);
        flow.set_homogeneous(true);
        scroll.add(&flow);
        root.pack_start(&scroll, true, true, 0);
        let hint = label("Click to copy, then paste with Ctrl+V", &["np-dim"]);
        hint.set_xalign(0.5);
        root.pack_start(&hint, false, false, 0);
        let recent = sys::read(recent_file())
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .take(RECENT_KEPT)
            .map(String::from)
            .collect();
        let me = Rc::new(Emoji {
            root,
            search,
            flow,
            hint,
            all: available(),
            recent,
            matches: RefCell::default(),
        });
        let weak = Rc::downgrade(&me);
        me.search.connect_search_changed(move |_| {
            if let Some(me) = weak.upgrade() {
                me.fill();
            }
        });
        let weak = Rc::downgrade(&me);
        me.search.connect_activate(move |_| {
            if let Some(me) = weak.upgrade() {
                let first = me.matches.borrow().first().cloned();
                if let Some(ch) = first {
                    me.pick(&ch);
                }
            }
        });
        me.fill();
        me
    }

    fn fill(self: &Rc<Self>) {
        for c in self.flow.children() {
            self.flow.remove(&c);
        }
        let query = self.search.text().to_lowercase();
        let matches = matching(&self.all, &self.recent, query.trim());
        for ch in matches.iter().take(SHOW) {
            let b = gtk::Button::with_label(ch);
            b.style_context().add_class("np-emoji");
            let name = self
                .all
                .iter()
                .find(|(c, _)| c == ch)
                .map_or("", |(_, n)| n.as_str());
            b.set_tooltip_text(Some(name));
            let (weak, ch) = (Rc::downgrade(self), ch.clone());
            b.connect_clicked(move |_| {
                if let Some(me) = weak.upgrade() {
                    me.pick(&ch);
                }
            });
            self.flow.add(&b);
        }
        self.flow.show_all();
        self.hint.set_text(if matches.is_empty() {
            "No matches."
        } else {
            "Click to copy, then paste with Ctrl+V"
        });
        *self.matches.borrow_mut() = matches;
    }

    fn pick(&self, ch: &str) {
        if !matches!(
            run("wl-copy", &[], Some(ch.as_bytes()), Duration::from_secs(5)),
            Ran::Ok(_)
        ) {
            return;
        }
        let mut recent = vec![ch.to_string()];
        recent.extend(self.recent.iter().filter(|c| *c != ch).cloned());
        recent.truncate(RECENT_KEPT);
        let file = recent_file();
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = sys::write_atomic(&file, &(recent.join("\n") + "\n"));
        notify("Emoji", &format!("Copied {ch}  Paste with Ctrl+V"));
        super::close();
    }
}

/// With a search: the emoji where every search word starts a word of the name
/// ("love" finds love, not glove). Without: recent ones first, then the rest.
fn matching(all: &[(String, String)], recent: &[String], query: &str) -> Vec<String> {
    if query.is_empty() {
        let have: HashSet<&str> = all.iter().map(|(c, _)| c.as_str()).collect();
        let mut out: Vec<String> = recent
            .iter()
            .filter(|c| have.contains(c.as_str()))
            .cloned()
            .collect();
        out.extend(
            all.iter()
                .filter(|(c, _)| !recent.contains(c))
                .map(|(c, _)| c.clone()),
        );
        return out;
    }
    let words: Vec<&str> = query.split_whitespace().collect();
    all.iter()
        .filter(|(_, name)| {
            words
                .iter()
                .all(|w| name.split_whitespace().any(|nw| nw.starts_with(w)))
        })
        .map(|(c, _)| c.clone())
        .collect()
}

/// The emoji list (emoji.txt) narrowed to what the emoji font can draw.
fn available() -> Vec<(String, String)> {
    let charset = match run(
        "fc-match",
        &["-f", "%{charset}", "Noto Color Emoji"],
        None,
        Duration::from_secs(5),
    ) {
        Ran::Ok(o) => String::from_utf8_lossy(&o).into_owned(),
        _ => String::new(),
    };
    let have = charset_ranges(&charset);
    include_str!("emoji.txt")
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let (hex, words) = l.split_once('\t')?;
            let cp = u32::from_str_radix(hex, 16).ok()?;
            if !have.is_empty() && !have.iter().any(|&(lo, hi)| lo <= cp && cp <= hi) {
                return None;
            }
            let mut ch = char::from_u32(cp)?.to_string();
            if cp < 0x1F000 {
                ch.push('\u{fe0f}'); // ask for the colour version
            }
            Some((ch, words.to_string()))
        })
        .collect()
}

/// fontconfig's charset ("20-7e a0-ff 1f600-1f64f ...") as ranges.
fn charset_ranges(text: &str) -> Vec<(u32, u32)> {
    text.split_whitespace()
        .filter_map(|part| {
            let (lo, hi) = part.split_once('-').unwrap_or((part, part));
            Some((
                u32::from_str_radix(lo, 16).ok()?,
                u32::from_str_radix(hi, 16).ok()?,
            ))
        })
        .collect()
}

impl Panel for Emoji {
    fn root(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }
    fn width(&self) -> i32 {
        440
    }
    fn anchor(&self) -> Anchor {
        Anchor::Middle
    }
    fn shown(self: Rc<Self>) {
        self.search.grab_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_and_order() {
        let all: Vec<(String, String)> = [
            ("😂", "face with tears of joy laugh"),
            ("🧤", "gloves"),
            ("❤\u{fe0f}", "heavy black heart love"),
        ]
        .iter()
        .map(|(c, n)| (c.to_string(), n.to_string()))
        .collect();
        assert_eq!(matching(&all, &[], "love"), ["❤\u{fe0f}"]);
        assert_eq!(matching(&all, &[], "glove"), ["🧤"]);
        assert_eq!(matching(&all, &[], "tears jo"), ["😂"]);
        assert_eq!(
            matching(&all, &["🧤".into(), "gone".into()], ""),
            ["🧤", "😂", "❤\u{fe0f}"]
        );
        assert_eq!(
            charset_ranges("20-7e 1f600 xyz"),
            [(0x20, 0x7e), (0x1f600, 0x1f600)]
        );
        assert!(include_str!("emoji.txt")
            .lines()
            .any(|l| l.starts_with("1F602\t")));
    }
}
