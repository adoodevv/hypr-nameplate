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

/// The program an app starts, if it names one. An app started over D-Bus, with
/// no `Exec=` line, has none: GLib answers NULL there, which the binding's
/// `AppInfo::executable()` doesn't allow for (it crashed the dock).
pub fn executable(app: &gtk::gio::AppInfo) -> Option<std::path::PathBuf> {
    use gtk::glib::translate::{from_glib_none, ToGlibPtr};
    // SAFETY: a live GAppInfo; the string it returns belongs to it and is
    // copied before `app` can go away.
    unsafe {
        let ptr = gtk::gio::ffi::g_app_info_get_executable(app.to_glib_none().0);
        (!ptr.is_null()).then(|| from_glib_none(ptr))
    }
}

#[cfg(test)]
mod tests {
    use gtk::gio;
    use gtk::prelude::*;

    #[test]
    fn app_without_exec_has_no_executable() {
        let dir = std::env::temp_dir().join(format!("np-exe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("org.example.DbusOnly.desktop");
        std::fs::write(&file, "[Desktop Entry]\nType=Application\nName=Dbus Only\nDBusActivatable=true\n").unwrap();
        let app = gio::DesktopAppInfo::from_filename(&file).expect("a desktop entry");
        assert_eq!(super::executable(app.upcast_ref()), None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
