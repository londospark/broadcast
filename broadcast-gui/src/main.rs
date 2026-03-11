mod app;
mod app_row;
mod window;

use adw::prelude::*;

const APP_ID: &str = "dev.dotfiles.broadcast";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let menu_mode = args.iter().any(|a| a == "--menu");
    // Strip --menu before GTK sees the args so it doesn't error on an unknown flag.
    let filtered_args: Vec<&str> = args.iter()
        .filter_map(|a| (a.as_str() != "--menu").then_some(a.as_str()))
        .collect();

    let app = adw::Application::builder().application_id(APP_ID).build();

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
