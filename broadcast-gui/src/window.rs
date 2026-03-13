use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gio, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use broadcast_core::backend::RealBackend;
use broadcast_core::state::{AppRoute, BroadcastState};
use broadcast_core::{filter, routing, AudioDevice};

use crate::app_row::AppRow;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct BroadcastWindow {
        pub master_switch: RefCell<Option<gtk::Switch>>,
        pub app_list: RefCell<Option<gtk::ListBox>>,
        pub state: RefCell<BroadcastState>,
        pub menu_mode: Cell<bool>,
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
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
                    gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl BroadcastWindow {
    /// Create a new window. Pass `Some((margin_top, margin_right))` for popup
    /// mode anchored to the top-right of the screen, or `None` for a regular window.
    pub fn new(app: &adw::Application, popup: Option<(i32, i32)>) -> Self {
        let menu_mode = popup.is_some();
        let (width, height) = if menu_mode { (420, 480) } else { (560, 520) };
        let win: Self = glib::Object::builder()
            .property("application", app)
            .property("title", "Broadcast")
            .property("default-width", width)
            .property("default-height", height)
            .build();

        win.imp().menu_mode.set(menu_mode);

        if let Some((margin_top, margin_right)) = popup {
            if gtk4_layer_shell::is_supported() {
                win.init_layer_shell();
                win.set_layer(Layer::Overlay);
                win.set_namespace(Some("broadcast-popup"));
                win.set_anchor(Edge::Top, true);
                win.set_anchor(Edge::Right, true);
                win.set_margin(Edge::Top, margin_top);
                win.set_margin(Edge::Right, margin_right);
                win.set_keyboard_mode(KeyboardMode::OnDemand);
            } else {
                win.set_decorated(false);
            }
            win.set_resizable(false);

            // Close the popup when it loses focus, but only after it has been
            // active at least once.  Without the guard the window can flash and
            // disappear immediately on wlroots compositors (Hyprland, niri, …)
            // because is_active toggles during the initial presentation before
            // focus has transferred from the bar surface.
            //
            // We also defer the close by 250 ms and re-check, because opening a
            // ComboRow dropdown popover on a layer-shell surface can cause a
            // transient focus loss that should not dismiss the window.
            let was_active = Rc::new(Cell::new(false));
            win.connect_is_active_notify(move |w| {
                if w.is_active() {
                    was_active.set(true);
                } else if was_active.get() {
                    let w = w.clone();
                    glib::timeout_add_local_once(std::time::Duration::from_millis(250), move || {
                        if !w.is_active() {
                            w.close();
                        }
                    });
                }
            });
        }

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
        let menu_mode = self.imp().menu_mode.get();

        // Global CSS: combo dropdown width + popup-mode styling
        let css = gtk::CssProvider::new();
        css.load_from_data(
            "popover > contents > scrolledwindow { min-width: 500px; }
             window.popup-mode.background {
                 background-color: rgba(230, 238, 255, 0.94);
                 border-radius: 14px;
             }",
        );
        gtk::style_context_add_provider_for_display(
            &gtk::prelude::WidgetExt::display(self),
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        // Header bar (omitted in menu/popup mode)
        let header = if menu_mode {
            self.add_css_class("popup-mode");
            None
        } else {
            Some(adw::HeaderBar::new())
        };

        // Master switch in header (or inline in mic row for popup mode)
        let master_switch = gtk::Switch::new();
        master_switch.set_active(state.active);
        master_switch.set_valign(gtk::Align::Center);
        master_switch.set_tooltip_text(Some("Master toggle — enable/disable all filtering"));
        if let Some(h) = &header {
            h.pack_end(&master_switch);
        }

        // Main content
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.set_margin_top(12);
        content.set_margin_bottom(12);
        content.set_margin_start(12);
        content.set_margin_end(12);

        // In popup mode, add a prominent master toggle at the top
        let master_row_opt = if menu_mode {
            let master_group = adw::PreferencesGroup::new();
            master_group.set_margin_bottom(18);
            let master_row = adw::ActionRow::builder()
                .title("Broadcast")
                .subtitle(if state.active { "Active" } else { "Off" })
                .build();
            let master_icon = gtk::Image::from_icon_name("audio-volume-high-symbolic");
            master_row.add_prefix(&master_icon);
            master_row.add_suffix(&master_switch);
            master_row.set_activatable_widget(Some(&master_switch));
            master_group.add(&master_row);
            content.append(&master_group);
            Some(master_row)
        } else {
            None
        };

        // Status section
        let status_group = adw::PreferencesGroup::builder()
            .title("Filter Status")
            .description("Noise suppression for mic input and app output")
            .build();
        status_group.set_margin_bottom(18);

        let mic_row = adw::ActionRow::builder()
            .title("Clean Mic")
            .subtitle("Microphone noise suppression")
            .build();
        let mic_icon = gtk::Image::from_icon_name("audio-input-microphone-symbolic");
        mic_row.add_prefix(&mic_icon);

        let mic_status = gtk::Label::new(Some(if state.active { "Active" } else { "Bypassed" }));
        mic_status.add_css_class(if state.active { "success" } else { "dim-label" });
        mic_row.add_suffix(&mic_status);

        let output_row = adw::ActionRow::builder()
            .title("App Output")
            .subtitle("Per-app output filtering")
            .build();
        let output_icon = gtk::Image::from_icon_name("audio-speakers-symbolic");
        output_row.add_prefix(&output_icon);

        let output_status =
            gtk::Label::new(Some(if state.active { "Filtered" } else { "Bypassed" }));
        output_status.add_css_class(if state.active { "success" } else { "dim-label" });
        output_row.add_suffix(&output_status);

        status_group.add(&mic_row);
        status_group.add(&output_row);
        content.append(&status_group);

        // Device selection section
        let device_group = adw::PreferencesGroup::builder()
            .title("Audio Devices")
            .description("Select which hardware devices to use")
            .build();
        device_group.set_margin_bottom(18);

        let backend = RealBackend;

        // Output device combo
        let output_devices =
            broadcast_core::list_output_devices(&backend, &state.nodes.output_sink)
                .unwrap_or_default();
        let output_combo = Self::build_device_combo(
            "Output Device",
            "Speakers / headphones for direct playback",
            &output_devices,
            state.preferred_output_sink.as_deref(),
        );
        device_group.add(&output_combo);

        // Input device combo
        let input_devices = broadcast_core::list_input_devices(&backend).unwrap_or_default();
        let input_combo = Self::build_device_combo(
            "Input Device",
            "Microphone fed into DeepFilterNet",
            &input_devices,
            state.preferred_input_source.as_deref(),
        );
        device_group.add(&input_combo);

        content.append(&device_group);

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
        let default_group = adw::PreferencesGroup::builder().title("Defaults").build();

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

        let default_label = gtk::Label::new(Some(if state.default_route == AppRoute::Filtered {
            "Filtered"
        } else {
            "Direct"
        }));
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
        if let Some(h) = &header {
            toolbar_view.add_top_bar(h);
        }
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
            let backend = RealBackend;
            let mut state = win.imp().state.borrow_mut();
            state.active = active;
            let _ = filter::set_filter_active(&backend, &state, active);
            if active {
                let _ = routing::apply_routes(&backend, &state);
            } else {
                let _ = routing::bypass_all(&backend, &state);
            }
            let _ = state.save();

            mic_status.set_text(if active { "Active" } else { "Bypassed" });
            output_status.set_text(if active { "Filtered" } else { "Bypassed" });
            for label in [&mic_status, &output_status] {
                if active {
                    label.remove_css_class("dim-label");
                    label.add_css_class("success");
                } else {
                    label.remove_css_class("success");
                    label.add_css_class("dim-label");
                }
            }
            if let Some(row) = &master_row_opt {
                row.set_subtitle(if active { "Active" } else { "Off" });
            }

            glib::Propagation::Proceed
        });

        // Connect default route switch
        let win = self.clone();
        default_switch.connect_state_set(move |_switch, active| {
            let mut state = win.imp().state.borrow_mut();
            state.default_route = if active {
                AppRoute::Filtered
            } else {
                AppRoute::Direct
            };
            default_label.set_text(if active { "Filtered" } else { "Direct" });
            let _ = state.save();
            glib::Propagation::Proceed
        });

        // Connect output device combo
        let win = self.clone();
        output_combo.connect_selected_notify(move |combo| {
            let idx = combo.selected();
            let mut state = win.imp().state.borrow_mut();
            if idx == 0 {
                state.set_preferred_output_sink(None);
            } else {
                let dev_idx = (idx - 1) as usize;
                if dev_idx < output_devices.len() {
                    state.set_preferred_output_sink(Some(output_devices[dev_idx].name.clone()));
                }
            }
            let _ = state.save();
            if state.active {
                let backend = RealBackend;
                let _ = routing::apply_routes(&backend, &state);
            }
        });

        // Connect input device combo
        let win = self.clone();
        input_combo.connect_selected_notify(move |combo| {
            let idx = combo.selected();
            let mut state = win.imp().state.borrow_mut();
            if idx == 0 {
                state.set_preferred_input_source(None);
            } else {
                let dev_idx = (idx - 1) as usize;
                if dev_idx < input_devices.len() {
                    state.set_preferred_input_source(Some(input_devices[dev_idx].name.clone()));
                }
            }
            let _ = state.save();
        });
    }

    fn build_device_combo(
        title: &str,
        subtitle: &str,
        devices: &[AudioDevice],
        current_pref: Option<&str>,
    ) -> adw::ComboRow {
        let model = gtk::StringList::new(&[]);
        model.append("(Auto-detect)");
        for dev in devices {
            model.append(&dev.description);
        }

        let selected: u32 = match current_pref {
            Some(pref) => devices
                .iter()
                .position(|d| d.name == pref)
                .map(|i| (i + 1) as u32)
                .unwrap_or(0),
            None => 0,
        };

        adw::ComboRow::builder()
            .title(title)
            .subtitle(subtitle)
            .model(&model)
            .selected(selected)
            .build()
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

        let backend = RealBackend;
        let state = imp.state.borrow();

        let apps = match routing::list_apps(&backend, &state) {
            Ok(apps) => apps,
            Err(_) => return,
        };

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
                let backend = RealBackend;
                let is_active = {
                    let mut state = win.imp().state.borrow_mut();
                    state.set_app_route(&binary, route);
                    let _ = state.save();
                    state.active
                };
                if is_active {
                    let state = win.imp().state.borrow();
                    let _ = routing::route_app(&backend, &state, &binary, route);
                }
            });

            app_list.append(&row.widget());
        }
    }
}
