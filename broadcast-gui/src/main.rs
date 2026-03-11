mod app;
mod app_row;
mod window;

use adw::prelude::*;

const APP_ID: &str = "dev.dotfiles.broadcast";

fn main() {
    let app = adw::Application::builder().application_id(APP_ID).build();

    app.connect_activate(|app| {
        if let Some(win) = app.windows().first() {
            win.present();
            return;
        }
        let win = window::BroadcastWindow::new(app);
        win.present();
    });

    app.run();
}
