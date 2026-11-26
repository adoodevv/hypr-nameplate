//! Clipboard history (SUPER + CTRL + V), from cliphist: click an entry (or
//! press Enter for the first match) to copy it again, then paste as usual.

use super::{background, clear, hbox, icon, label, notify, row, run, vbox, Anchor, Panel, Ran};
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;

const LIMIT: usize = 60;
const TIMEOUT: Duration = Duration::from_secs(10);

pub struct Clipboard {
    root: gtk::Box,
    search: gtk::SearchEntry,
    list: gtk::Box,
    /// cliphist's lines ("id<TAB>preview"), raw: a preview cut mid-character
    /// still decodes, because cliphist gets back exactly what it gave.
    entries: RefCell<Vec<Vec<u8>>>,
    missing: Cell<bool>,
}

impl Clipboard {
    pub fn new() -> Rc<Self> {
        let root = vbox(6);
        let head = hbox(8);
        head.pack_start(&label("Clipboard history", &["np-title"]), true, true, 0);
        root.pack_start(&head, false, false, 0);

        let search = gtk::SearchEntry::new();
        search.style_context().add_class("np-search");
        search.set_placeholder_text(Some("Search"));
        root.pack_start(&search, false, false, 0);

        let scroll = gtk::ScrolledWindow::new(gtk::Adjustment::NONE, gtk::Adjustment::NONE);
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroll.set_min_content_height(360);
        let list = vbox(0);
        scroll.add(&list);
        root.pack_start(&scroll, true, true, 0);
        root.pack_start(
            &gtk::Separator::new(gtk::Orientation::Horizontal),
            false,
            false,
            0,
        );

        let me = Rc::new(Clipboard {
            root,
            search,
            list,
            entries: RefCell::default(),
            missing: Cell::new(false),
        });

        let foot = hbox(8);
        foot.pack_start(
            &label("Click to copy, then paste with Ctrl+V", &["np-dim"]),
            true,
            true,
            0,
        );
        let weak = Rc::downgrade(&me);
        foot.pack_end(
            &row(icon::TRASH, "Clear", "", move || {
                if let Some(me) = weak.upgrade() {
                    me.clear_all();
                }
            }),
            false,
            false,
            0,
        );
        me.root.pack_start(&foot, false, false, 0);

        let weak = Rc::downgrade(&me);
        me.search.connect_search_changed(move |_| {
            if let Some(me) = weak.upgrade() {
                me.fill();
            }
        });
        let weak = Rc::downgrade(&me);
        me.search.connect_activate(move |_| {
            if let Some(me) = weak.upgrade() {
                me.pick_first();
            }
        });
        me.load();
        me
    }

    fn load(self: &Rc<Self>) {
        let (entries, missing) = match run("cliphist", &["list"], None, TIMEOUT) {
            Ran::Ok(out) => (
                out.split(|&b| b == b'\n')
                    .filter(|l| l.contains(&b'\t')) // a warning isn't an entry
                    .map(<[u8]>::to_vec)
                    .collect(),
                false,
            ),
            Ran::Missing => (Vec::new(), true),
            _ => (Vec::new(), false),
        };
        *self.entries.borrow_mut() = entries;
        self.missing.set(missing);
        self.fill();
    }

    fn query(&self) -> String {
        self.search.text().to_lowercase()
    }

    fn fill(self: &Rc<Self>) {
        clear(&self.list);
        if self.missing.get() {
            self.list.pack_start(
                &label(
                    "cliphist isn't installed. Run the installer again.",
                    &["np-dim"],
                ),
                false,
                false,
                6,
            );
        }
        let query = self.query();
        let mut shown = 0;
        for line in self.entries.borrow().iter() {
            let preview = preview(line);
            if !query.is_empty() && !preview.to_lowercase().contains(&query) {
                continue;
            }
            let (glyph, text) = describe(&preview);
            let weak: Weak<Self> = Rc::downgrade(self);
            let line = line.clone();
            let text = if text.is_empty() {
                "(empty)".into()
            } else {
                text
            };
            self.list.pack_start(
                &row(glyph, &text, "", move || {
                    if let Some(me) = weak.upgrade() {
                        me.pick(&line);
                    }
                }),
                false,
                false,
                0,
            );
            shown += 1;
            if shown >= LIMIT {
                break;
            }
        }
        if shown == 0 && !self.missing.get() {
            let text = if query.is_empty() {
                "Nothing yet. Things you copy show up here."
            } else {
                "No matches."
            };
            self.list
                .pack_start(&label(text, &["np-dim"]), false, false, 6);
        }
        self.list.show_all();
    }

    fn pick_first(self: &Rc<Self>) {
        let query = self.query();
        let first = self
            .entries
            .borrow()
            .iter()
            .find(|l| query.is_empty() || preview(l).to_lowercase().contains(&query))
            .cloned();
        if let Some(line) = first {
            self.pick(&line);
        }
    }

    fn pick(&self, line: &[u8]) {
        let mut input = line.to_vec();
        input.push(b'\n');
        let Ran::Ok(data) = run("cliphist", &["decode"], Some(&input), TIMEOUT) else {
            return;
        };
        // wl-copy stays behind to serve the paste; it mustn't hold the panel up.
        background(move || run("wl-copy", &[], Some(&data), TIMEOUT), |_| ());
        notify("Clipboard", "Copied. Paste with Ctrl+V");
        super::close();
    }

    fn clear_all(self: &Rc<Self>) {
        run("cliphist", &["wipe"], None, TIMEOUT);
        self.load();
    }
}

/// The text after cliphist's id, readable even if it isn't valid UTF-8.
fn preview(line: &[u8]) -> String {
    let at = line.iter().position(|&b| b == b'\t').map_or(0, |i| i + 1);
    String::from_utf8_lossy(&line[at..]).into_owned()
}

/// The row's icon and text: images as "Image  1920x1080  (png)", text with its
/// runs of spaces and newlines folded into single spaces.
fn describe(preview: &str) -> (&'static str, String) {
    if let Some(rest) = preview.strip_prefix("[[ binary data ") {
        if let Some(inner) = rest.split(" ]]").next().filter(|_| rest.contains(" ]]")) {
            let words: Vec<&str> = inner.split_whitespace().collect();
            if let [.., format, size] = words.as_slice() {
                let is_size = size.split_once('x').is_some_and(|(w, h)| {
                    !w.is_empty()
                        && !h.is_empty()
                        && w.bytes().all(|b| b.is_ascii_digit())
                        && h.bytes().all(|b| b.is_ascii_digit())
                });
                if is_size && format.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    return (icon::IMAGE, format!("Image  {size}  ({format})"));
                }
            }
        }
    }
    let glyph = if preview.starts_with("[[ binary") {
        icon::IMAGE
    } else {
        icon::CLIP
    };
    (
        glyph,
        preview.split_whitespace().collect::<Vec<_>>().join(" "),
    )
}

impl Panel for Clipboard {
    fn root(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }
    fn width(&self) -> i32 {
        420
    }
    fn anchor(&self) -> Anchor {
        Anchor::Middle
    }
    fn shown(&self) {
        self.search.grab_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_entries() {
        assert_eq!(
            describe("[[ binary data 52 KiB png 1920x1080 ]]"),
            (icon::IMAGE, "Image  1920x1080  (png)".into())
        );
        assert_eq!(describe("[[ binary data 3 KiB ]]").0, icon::IMAGE);
        assert_eq!(
            describe("  hello\n  world\t!"),
            (icon::CLIP, "hello world !".into())
        );
        assert_eq!(preview(b"12\tab\xffc"), "ab\u{fffd}c");
    }
}
