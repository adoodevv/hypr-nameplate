//! Small GTK helpers shared by the wallpaper and the dock.

use gtk::prelude::*;
use gtk::{gdk, glib};

pub fn add_css(css: &str) {
    let provider = gtk::CssProvider::new();
    if provider.load_from_data(css.as_bytes()).is_ok() {
        if let Some(screen) = gdk::Screen::default() {
            gtk::StyleContext::add_provider_for_screen(
                &screen,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_USER,
            );
        }
    }
}

/// Give a window an alpha channel, so transparent areas really are see-through.
pub fn transparent(window: &gtk::Window) {
    if let Some(visual) = WidgetExt::screen(window).and_then(|s| s.rgba_visual()) {
        window.set_visual(Some(&visual));
    }
}

/// Run `f` on the GTK main loop from any thread. The closure may only carry
/// `Send` data; anything GTK-side is reached through thread-local state.
pub fn on_main<F: FnOnce() + Send + 'static>(f: F) {
    glib::MainContext::default().invoke(f);
}
