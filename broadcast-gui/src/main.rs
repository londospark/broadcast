mod app;
mod window;
mod app_row;

use adw::prelude::*;

const APP_ID: &str = "dev.dotfiles.broadcast";

fn main() {
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .build();

    app.connect_activate(|app| {
        let win = window::BroadcastWindow::new(app);
        win.present();
    });

    app.run();
}
