use gtk::prelude::*;
use gtk::glib;
use adw::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

use broadcast_core::state::AppRoute;

/// A row in the app list showing an app name with a filter toggle.
pub struct AppRow {
    row: adw::ActionRow,
    switch: gtk::Switch,
    on_route_changed: Rc<RefCell<Option<Box<dyn Fn(AppRoute)>>>>,
}

impl AppRow {
    pub fn new(name: &str, binary: &str, media: &str, current_route: AppRoute) -> Self {
        let row = adw::ActionRow::builder()
            .title(name)
            .subtitle(if !media.is_empty() { media } else { binary })
            .build();

        // App icon based on common app names
        let icon_name = match binary.to_lowercase().as_str() {
            s if s.contains("brave") => "web-browser-symbolic",
            s if s.contains("firefox") => "web-browser-symbolic",
            s if s.contains("chromium") || s.contains("chrome") => "web-browser-symbolic",
            s if s.contains("spotify") => "multimedia-audio-player-symbolic",
            s if s.contains("steam") => "input-gaming-symbolic",
            s if s.contains("discord") => "user-available-symbolic",
            _ => "multimedia-volume-control-symbolic",
        };
        let icon = gtk::Image::from_icon_name(icon_name);
        row.add_prefix(&icon);

        let switch = gtk::Switch::new();
        switch.set_active(current_route == AppRoute::Filtered);
        switch.set_valign(gtk::Align::Center);
        switch.set_tooltip_text(Some("On = filtered through DeepFilterNet, Off = direct to speakers"));
        row.add_suffix(&switch);
        row.set_activatable_widget(Some(&switch));

        let label = gtk::Label::new(Some(
            if current_route == AppRoute::Filtered { "Filtered" } else { "Direct" }
        ));
        label.add_css_class("dim-label");
        row.add_suffix(&label);

        let on_route_changed: Rc<RefCell<Option<Box<dyn Fn(AppRoute)>>>> =
            Rc::new(RefCell::new(None));

        let cb = on_route_changed.clone();
        switch.connect_state_set(move |_switch, active| {
            let route = if active { AppRoute::Filtered } else { AppRoute::Direct };
            label.set_text(if active { "Filtered" } else { "Direct" });
            if let Some(ref f) = *cb.borrow() {
                f(route);
            }
            glib::Propagation::Proceed
        });

        Self {
            row,
            switch,
            on_route_changed,
        }
    }

    pub fn widget(&self) -> adw::ActionRow {
        self.row.clone()
    }

    pub fn connect_route_changed<F: Fn(AppRoute) + 'static>(&self, f: F) {
        *self.on_route_changed.borrow_mut() = Some(Box::new(f));
    }
}
