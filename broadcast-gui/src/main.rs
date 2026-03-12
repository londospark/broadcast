mod app;
mod app_row;
mod window;

use adw::prelude::*;
use gtk::gio;

const APP_ID: &str = "dev.dotfiles.broadcast";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let menu_mode = args.iter().any(|a| a == "--menu");
    // Strip --menu before GTK sees the args so it doesn't error on an unknown flag.
    let filtered_args: Vec<&str> = args.iter()
        .filter_map(|a| (a.as_str() != "--menu").then_some(a.as_str()))
        .collect();

    // In menu mode each invocation must be its own process: if broadcast-gui was
    // already running as a normal window the GApplication single-instance mechanism
    // would forward the activate signal to that process with no argument payload,
    // silently ignoring --menu.  NON_UNIQUE prevents that.
    let flags = if menu_mode {
        gio::ApplicationFlags::NON_UNIQUE
    } else {
        gio::ApplicationFlags::empty()
    };

    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(flags)
        .build();

    app.connect_activate(move |app| {
        if !menu_mode {
            if let Some(win) = app.windows().first() {
                win.present();
                return;
            }
        }
        let win = window::BroadcastWindow::new(app, menu_mode);
        win.present();
    });

    app.run_with_args(&filtered_args);
}
