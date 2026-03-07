use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gio, glib};
use std::cell::RefCell;

use broadcast_core::state::{AppRoute, BroadcastState};
use broadcast_core::{filter, routing};

use crate::app_row::AppRow;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct BroadcastWindow {
        pub master_switch: RefCell<Option<gtk::Switch>>,
        pub app_list: RefCell<Option<gtk::ListBox>>,
        pub state: RefCell<BroadcastState>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for BroadcastWindow {
        const NAME: &'static str = "BroadcastWindow";
        type Type = super::BroadcastWindow;
        type ParentType = adw::ApplicationWindow;
    }

    impl ObjectImpl for BroadcastWindow {}
    impl WidgetImpl for BroadcastWindow {}
    impl WindowImpl for BroadcastWindow {}
    impl ApplicationWindowImpl for BroadcastWindow {}
    impl AdwApplicationWindowImpl for BroadcastWindow {}
}

glib::wrapper! {
    pub struct BroadcastWindow(ObjectSubclass<imp::BroadcastWindow>)
        @extends adw::ApplicationWindow, gtk::ApplicationWindow, gtk::Window, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl BroadcastWindow {
    pub fn new(app: &adw::Application) -> Self {
        let win: Self = glib::Object::builder()
            .property("application", app)
            .property("title", "Broadcast")
            .property("default-width", 420)
            .property("default-height", 520)
            .build();

        win.setup_ui();
        win.refresh_apps();

        // Poll for stream changes every 3 seconds
        let win_weak = win.downgrade();
        glib::timeout_add_seconds_local(3, move || {
            if let Some(win) = win_weak.upgrade() {
                win.refresh_apps();
                glib::ControlFlow::Continue
            } else {
                glib::ControlFlow::Break
            }
        });

        win
    }

    fn setup_ui(&self) {
        let state = BroadcastState::load().unwrap_or_default();

        // Header bar
        let header = adw::HeaderBar::new();

        // Master switch in header
        let master_switch = gtk::Switch::new();
        master_switch.set_active(state.master);
        master_switch.set_valign(gtk::Align::Center);
        master_switch.set_tooltip_text(Some("Master toggle — enable/disable all filtering"));
        header.pack_end(&master_switch);

        // Main content
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.set_margin_top(12);
        content.set_margin_bottom(12);
        content.set_margin_start(12);
        content.set_margin_end(12);

        // Mic section
        let mic_group = adw::PreferencesGroup::builder()
            .title("Microphone Input")
            .description("Real mic → DeepFilterNet → Clean Mic")
            .build();
        mic_group.set_margin_bottom(18);

        let mic_row = adw::ActionRow::builder()
            .title("Clean Mic")
            .subtitle("Noise suppression on microphone")
            .build();
        let mic_icon = gtk::Image::from_icon_name("audio-input-microphone-symbolic");
        mic_row.add_prefix(&mic_icon);

        let mic_status = gtk::Label::new(Some(if state.master { "Active" } else { "Bypassed" }));
        mic_status.add_css_class(if state.master { "success" } else { "dim-label" });
        mic_row.add_suffix(&mic_status);

        mic_group.add(&mic_row);
        content.append(&mic_group);

        // App list section
        let app_group = adw::PreferencesGroup::builder()
            .title("Application Output")
            .description("Choose which apps get noise filtering")
            .build();

        let app_list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(vec!["boxed-list".to_string()])
            .build();

        app_group.add(&app_list);
        app_group.set_margin_bottom(18);
        content.append(&app_group);

        // Default route section
        let default_group = adw::PreferencesGroup::builder()
            .title("Defaults")
            .build();

        let default_row = adw::ActionRow::builder()
            .title("New apps default to")
            .subtitle("Applied when a new audio stream appears")
            .build();

        let default_switch = gtk::Switch::new();
        default_switch.set_active(state.default_route == AppRoute::Filtered);
        default_switch.set_valign(gtk::Align::Center);
        default_switch.set_tooltip_text(Some("On = filtered, Off = direct"));
        default_row.add_suffix(&default_switch);
        default_row.set_activatable_widget(Some(&default_switch));

        let default_label = gtk::Label::new(Some(
            if state.default_route == AppRoute::Filtered { "Filtered" } else { "Direct" }
        ));
        default_label.add_css_class("dim-label");
        default_row.add_suffix(&default_label);

        default_group.add(&default_row);
        content.append(&default_group);

        // Scrolled container
        let scrolled = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .child(&content)
            .vexpand(true)
            .build();

        let toolbar_view = adw::ToolbarView::new();
        toolbar_view.add_top_bar(&header);
        toolbar_view.set_content(Some(&scrolled));

        self.set_content(Some(&toolbar_view));

        // Store refs
        let imp = self.imp();
        *imp.master_switch.borrow_mut() = Some(master_switch.clone());
        *imp.app_list.borrow_mut() = Some(app_list);
        *imp.state.borrow_mut() = state;

        // Connect master switch
        let win = self.clone();
        master_switch.connect_state_set(move |_switch, active| {
            let mut state = win.imp().state.borrow_mut();
            state.master = active;
            let _ = filter::set_filter_active(active);
            if active {
                let _ = routing::apply_routes(&state.app_routes, state.default_route);
            }
            let _ = state.save();

            mic_status.set_text(if active { "Active" } else { "Bypassed" });
            if active {
                mic_status.remove_css_class("dim-label");
                mic_status.add_css_class("success");
            } else {
                mic_status.remove_css_class("success");
                mic_status.add_css_class("dim-label");
            }

            glib::Propagation::Proceed
        });

        // Connect default route switch
        let win = self.clone();
        default_switch.connect_state_set(move |_switch, active| {
            let mut state = win.imp().state.borrow_mut();
            state.default_route = if active { AppRoute::Filtered } else { AppRoute::Direct };
            default_label.set_text(if active { "Filtered" } else { "Direct" });
            let _ = state.save();
            glib::Propagation::Proceed
        });
    }

    fn refresh_apps(&self) {
        let imp = self.imp();
        let app_list = imp.app_list.borrow();
        let app_list = match app_list.as_ref() {
            Some(list) => list,
            None => return,
        };

        // Clear existing rows
        while let Some(child) = app_list.first_child() {
            app_list.remove(&child);
        }

        let apps = match routing::list_apps() {
            Ok(apps) => apps,
            Err(_) => return,
        };

        let state = imp.state.borrow();

        for app in &apps {
            let name = if !app.name.is_empty() {
                &app.name
            } else {
                &app.binary
            };

            let saved_route = state.route_for(&app.binary);
            let row = AppRow::new(name, &app.binary, &app.media, saved_route);

            let win = self.clone();
            let binary = app.binary.clone();
            row.connect_route_changed(move |route| {
                let mut state = win.imp().state.borrow_mut();
                state.set_app_route(&binary, route);
                let _ = state.save();
                if state.master {
                    let _ = routing::route_app(&binary, route);
                }
            });

            app_list.append(&row.widget());
        }
    }
}
