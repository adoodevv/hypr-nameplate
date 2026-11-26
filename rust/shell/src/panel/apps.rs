//! The app grid (SUPER + ALT + SPACE), laid out like Omarchy 4's: a card in the
//! middle of the screen, seven columns of large icons, a search line on top.
//! Start typing to search; arrows and Tab move, Enter opens, Escape clears the
//! search and then closes. Right-click an app to pin it to the dock.

use super::{label, vbox, Anchor, Panel};
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use std::cell::RefCell;
use std::rc::Rc;

const COLUMNS: i32 = 7;
const ROWS: i32 = 4;
const ICON: i32 = 56;
const CELL_W: i32 = 124;
const CELL_H: i32 = 128;

struct App {
    info: gio::AppInfo,
    /// Everything a search can match, lower-cased.
    words: String,
}

pub struct Apps {
    root: gtk::Box,
    search: gtk::Entry,
    grid: gtk::FlowBox,
    empty: gtk::Label,
    apps: Rc<Vec<App>>,
    /// The cells that match the search, in order.
    visible: RefCell<Vec<gtk::FlowBoxChild>>,
}

impl Apps {
    pub fn new() -> Rc<Self> {
        let root = vbox(18);
        root.style_context().add_class("np-apps");
        let search = gtk::Entry::new();
        search.style_context().add_class("np-apps-search");
        search.set_placeholder_text(Some("Search apps"));
        root.pack_start(&search, false, false, 0);

        let grid = gtk::FlowBox::new();
        grid.set_homogeneous(true);
        grid.set_min_children_per_line(COLUMNS as u32);
        grid.set_max_children_per_line(COLUMNS as u32);
        grid.set_selection_mode(gtk::SelectionMode::Single);
        grid.set_activate_on_single_click(true);
        grid.set_can_focus(false);
        let scroll = gtk::ScrolledWindow::new(gtk::Adjustment::NONE, gtk::Adjustment::NONE);
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroll.set_size_request(-1, ROWS * CELL_H);
        scroll.add(&grid);
        // The card keeps the size it opened at (every app showing) while a search
        // narrows the grid, like Omarchy's, instead of shrinking around the matches.
        scroll.connect_size_allocate(|scroll, alloc| {
            if alloc.width() > scroll.size_request().0 {
                scroll.set_size_request(alloc.width(), ROWS * CELL_H);
            }
        });
        let empty = label("No apps match", &["np-dim"]);
        empty.set_halign(gtk::Align::Center);
        empty.set_valign(gtk::Align::Center);
        empty.set_no_show_all(true);
        let overlay = gtk::Overlay::new();
        overlay.add(&scroll);
        overlay.add_overlay(&empty);
        root.pack_start(&overlay, true, true, 0);

        let mut apps: Vec<App> = gio::AppInfo::all()
            .into_iter()
            .filter(|a| a.should_show())
            .map(|info| App {
                words: words(&info),
                info,
            })
            .collect();
        apps.sort_by_cached_key(|a| a.info.display_name().to_lowercase());
        let apps = Rc::new(apps);

        let me = Rc::new(Apps {
            root,
            search,
            grid,
            empty,
            apps: apps.clone(),
            visible: RefCell::default(),
        });

        let theme = gtk::IconTheme::default();
        for (i, app) in apps.iter().enumerate() {
            me.grid.add(&me.cell(i, app, theme.as_ref()));
        }

        let (search, list) = (me.search.clone(), apps.clone());
        me.grid.set_filter_func(Some(Box::new(move |child| {
            matches(&list[child.index() as usize].words, &search.text())
        })));
        let weak = Rc::downgrade(&me);
        me.grid.connect_child_activated(move |_, child| {
            if let Some(me) = weak.upgrade() {
                me.launch(Some(child));
            }
        });
        let weak = Rc::downgrade(&me);
        me.search.connect_changed(move |_| {
            if let Some(me) = weak.upgrade() {
                me.refilter();
            }
        });
        let weak = Rc::downgrade(&me);
        me.search.connect_activate(move |_| {
            if let Some(me) = weak.upgrade() {
                me.launch(me.selected().as_ref());
            }
        });
        me
    }

    fn cell(
        self: &Rc<Self>,
        index: usize,
        app: &App,
        theme: Option<&gtk::IconTheme>,
    ) -> gtk::FlowBoxChild {
        let child = gtk::FlowBoxChild::new();
        child.set_size_request(CELL_W, CELL_H);
        child.style_context().add_class("np-app");
        let bx = vbox(8);
        bx.set_valign(gtk::Align::Start);

        let image = gtk::Image::new();
        if let Some(pix) = theme.and_then(|t| icon(t, &app.info)) {
            image.set_from_pixbuf(Some(&pix));
        }
        image.set_size_request(ICON, ICON);
        bx.pack_start(&image, false, false, 0);

        let name = label(&app.info.display_name(), &["np-app-name"]);
        name.set_xalign(0.5);
        name.set_justify(gtk::Justification::Center);
        name.set_line_wrap(true);
        name.set_lines(2);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        name.set_max_width_chars(12);
        name.set_halign(gtk::Align::Center);
        bx.pack_start(&name, false, false, 0);

        let events = gtk::EventBox::new();
        events.add(&bx);
        let weak = Rc::downgrade(self);
        events.connect_button_press_event(move |_, ev| {
            if ev.button() == 3 {
                if let Some(me) = weak.upgrade() {
                    me.menu(index, ev);
                    return glib::Propagation::Stop;
                }
            }
            glib::Propagation::Proceed
        });
        child.add(&events);
        let tip = app
            .info
            .description()
            .map(|d| d.to_string())
            .unwrap_or_else(|| app.info.display_name().to_string());
        child.set_tooltip_text(Some(&tip));
        child
    }

    /// Right-click: pin the app to the dock or take it off, like Omarchy's grid.
    fn menu(&self, index: usize, event: &gdk::EventButton) {
        let Some(id) = self.apps[index].info.id().map(|s| s.to_string()) else {
            return;
        };
        let pinned = crate::dock::pinned().contains(&id);
        let menu = gtk::Menu::new();
        let item = gtk::MenuItem::with_label(if pinned {
            "Unpin from dock"
        } else {
            "Pin to dock"
        });
        item.connect_activate(move |_| crate::dock::set_pinned(&id, !pinned));
        menu.append(&item);
        menu.show_all();
        menu.set_attach_widget(Some(&self.grid));
        menu.popup_at_pointer(Some(event));
    }

    fn refilter(&self) {
        self.grid.invalidate_filter();
        let query = self.search.text();
        let visible: Vec<gtk::FlowBoxChild> = self
            .grid
            .children()
            .into_iter()
            .filter_map(|w| w.downcast::<gtk::FlowBoxChild>().ok())
            .filter(|c| matches(&self.apps[c.index() as usize].words, &query))
            .collect();
        self.empty.set_visible(visible.is_empty());
        match visible.first() {
            Some(first) => self.grid.select_child(first),
            None => self.grid.unselect_all(),
        }
        *self.visible.borrow_mut() = visible;
    }

    fn selected(&self) -> Option<gtk::FlowBoxChild> {
        self.grid.selected_children().into_iter().next()
    }

    fn launch(&self, child: Option<&gtk::FlowBoxChild>) {
        let Some(child) = child else {
            return;
        };
        let info = &self.apps[child.index() as usize].info;
        match info.id() {
            // A session of its own, not a child of the panel.
            Some(id) if nameplate::sys::which("gtk-launch").is_some() => {
                nameplate::sys::spawn("gtk-launch", &[id.as_str()]);
            }
            _ => {
                let _ = info.launch(&[], gio::AppLaunchContext::NONE);
            }
        }
        super::close();
    }
}

impl Panel for Apps {
    fn root(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }
    fn width(&self) -> i32 {
        COLUMNS * CELL_W + 16
    }
    fn anchor(&self) -> Anchor {
        Anchor::Middle
    }
    fn dim_backdrop(&self) -> bool {
        true
    }
    fn shown(&self) {
        self.refilter();
        self.search.grab_focus();
    }
    fn on_key(&self, event: &gdk::EventKey) -> bool {
        use gdk::keys::constants as key;
        let k = event.keyval();
        if k == key::Escape && !self.search.text().is_empty() {
            self.search.set_text("");
            return true;
        }
        let step = if k == key::Left || k == key::ISO_Left_Tab {
            -1
        } else if k == key::Right || k == key::Tab {
            1
        } else if k == key::Up {
            -COLUMNS
        } else if k == key::Down {
            COLUMNS
        } else {
            return false;
        };
        let visible = self.visible.borrow();
        if visible.is_empty() {
            return false;
        }
        let now = self.selected();
        let i = now
            .and_then(|n| visible.iter().position(|c| *c == n))
            .unwrap_or(0) as i32;
        let i = (i + step).clamp(0, visible.len() as i32 - 1) as usize;
        self.grid.select_child(&visible[i]);
        visible[i].grab_focus();
        self.search.grab_focus_without_selecting();
        true
    }
}

/// Every word of the search is somewhere in the app's words.
fn matches(words: &str, query: &str) -> bool {
    query
        .trim()
        .to_lowercase()
        .split_whitespace()
        .all(|w| words.contains(w))
}

fn words(info: &gio::AppInfo) -> String {
    let mut parts: Vec<String> = vec![info.display_name().to_string(), info.name().to_string()];
    if let Some(d) = info.description() {
        parts.push(d.to_string());
    }
    if let Some(desktop) = info.downcast_ref::<gio::DesktopAppInfo>() {
        parts.extend(desktop.keywords().iter().map(|k| k.to_string()));
    }
    parts.push(info.executable().to_string_lossy().into_owned());
    parts
        .into_iter()
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn icon(theme: &gtk::IconTheme, info: &gio::AppInfo) -> Option<gtk::gdk_pixbuf::Pixbuf> {
    let flags = gtk::IconLookupFlags::FORCE_SIZE;
    info.icon()
        .and_then(|i| theme.lookup_by_gicon(&i, ICON, flags))
        .and_then(|found| found.load_icon().ok())
        .or_else(|| {
            theme
                .load_icon("application-x-executable", ICON, flags)
                .ok()
                .flatten()
        })
}

#[cfg(test)]
mod tests {
    use super::matches;

    #[test]
    fn search_matches_every_word() {
        let words = "firefox web browser browse the world wide web firefox";
        assert!(matches(words, ""));
        assert!(matches(words, "  Web  FIRE "));
        assert!(!matches(words, "web mail"));
    }
}
