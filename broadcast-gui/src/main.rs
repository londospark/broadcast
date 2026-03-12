mod app;
mod app_row;
mod window;

use adw::prelude::*;
use gtk::gio;

const APP_ID: &str = "dev.dotfiles.broadcast";

fn parse_int_arg(args: &[String], flag: &str) -> Option<i32> {
    args.windows(2)
        .find(|w| w[0] == flag)
        .and_then(|w| w[1].parse().ok())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let menu_mode = args.iter().any(|a| a == "--menu");
    let margin_top = parse_int_arg(&args, "--margin-top").unwrap_or(48);
    let margin_right = parse_int_arg(&args, "--margin-right").unwrap_or(10);

    let popup = if menu_mode {
        Some((margin_top, margin_right))
    } else {
        None
    };

    // Strip our flags before GTK sees the args.
    let custom_flags = ["--menu", "--margin-top", "--margin-right"];
    let mut filtered_args: Vec<&str> = Vec::new();
    let mut skip_next = false;
    for a in &args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if a == "--margin-top" || a == "--margin-right" {
            skip_next = true;
            continue;
        }
        if !custom_flags.contains(&a.as_str()) {
            filtered_args.push(a);
        }
    }

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
        if popup.is_none() {
            if let Some(win) = app.windows().first() {
                win.present();
                return;
            }
        }
        let win = window::BroadcastWindow::new(app, popup);
        win.present();
    });

    app.run_with_args(&filtered_args);
}
