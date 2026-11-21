use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use crossbeam_channel::Receiver;
use gtk4::glib;
use gtk4::prelude::*;
use libadwaita::prelude::*;

use crate::backend::MouseButton;
use crate::core::click_engine::{ClickEngine, EngineCommand, EngineSettings};
use crate::core::scheduler::ClickMode;
use crate::system::hotkey::{self, HotkeyListener};
use crate::utils::config::AppConfig;

use super::window::MainWindow;

type CaptureResultRx = Rc<RefCell<Option<Receiver<Result<u16, String>>>>>;

pub struct AppController {
    engine: RefCell<ClickEngine>,
    hotkey: RefCell<HotkeyListener>,
    config: RefCell<AppConfig>,
    config_dirty: RefCell<bool>,
    running: RefCell<bool>,
    capture_shutdown: RefCell<Option<Arc<AtomicBool>>>,
}

impl AppController {
    pub fn new(
        window: &MainWindow,
        config: AppConfig,
        engine: ClickEngine,
        hotkey: HotkeyListener,
    ) -> Rc<Self> {
        let controller = Rc::new(Self {
            engine: RefCell::new(engine),
            hotkey: RefCell::new(hotkey),
            config: RefCell::new(config),
            config_dirty: RefCell::new(false),
            running: RefCell::new(false),
            capture_shutdown: RefCell::new(None),
        });

        let cps = controller.config.borrow().cps.default;
        window.set_cps_display(cps);
        window.cps_spin.set_value(cps as f64);
        window.set_hotkey_display(&controller.config.borrow().hotkey.key);

        controller.wire_signals(window);
        controller.start_hotkey(&controller.config.borrow().hotkey.key.clone());
        controller.start_stats_poll(window);

        // Debounced config auto-save (500ms idle)
        {
            let this = Rc::clone(&controller);
            glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
                if *this.config_dirty.borrow() {
                    if let Err(e) = this.config.borrow().save() {
                        tracing::warn!("Failed to save config: {e}");
                    }
                    *this.config_dirty.borrow_mut() = false;
                }
                glib::ControlFlow::Continue
            });
        }

        controller
    }

    pub fn shutdown(&self) {
        if let Some(flag) = self.capture_shutdown.borrow_mut().take() {
            flag.store(true, Ordering::SeqCst);
        }
        if *self.config_dirty.borrow() {
            if let Err(e) = self.config.borrow().save() {
                tracing::warn!("Failed to save config on shutdown: {e}");
            }
            *self.config_dirty.borrow_mut() = false;
        }
        self.hotkey.borrow_mut().stop();
        self.engine.borrow_mut().shutdown();
    }

    fn wire_signals(self: &Rc<Self>, window: &MainWindow) {
        let this = Rc::clone(self);

        // CPS spin row
        window.cps_spin.connect_notify_local(Some("value"), {
            let this = Rc::clone(&this);
            let label = window.cps_value_label.clone();
            move |spin, _| {
                let cps = spin.value().round() as u32;
                label.set_text(&format!("{cps} CPS"));
                this.engine.borrow().send(EngineCommand::SetCps(cps));
                this.config.borrow_mut().cps.default = cps;
                *this.config_dirty.borrow_mut() = true;
            }
        });

        // CPS Preset buttons
        for (btn, value) in [
            (&window.preset_10, 10),
            (&window.preset_20, 20),
            (&window.preset_50, 50),
            (&window.preset_100, 100),
        ] {
            let spin = window.cps_spin.clone();
            let val = value;
            btn.connect_clicked(move |_| {
                spin.set_value(val as f64);
            });
        }

        window.start_stop_btn.connect_clicked({
            let this = Rc::clone(&this);
            move |_| {
                let running = *this.running.borrow();
                if running {
                    this.engine.borrow().send(EngineCommand::Stop);
                } else {
                    this.engine.borrow().send(EngineCommand::Start);
                }
            }
        });

        window.mode_row.connect_selected_notify({
            let this = Rc::clone(&this);
            let burst_row = window.burst_row.clone();
            move |row| {
                let idx = row.selected();
                let mode = ClickMode::from_index(idx);
                burst_row.set_visible(mode == ClickMode::Burst);
                this.engine.borrow().send(EngineCommand::SetMode(mode));
                this.config.borrow_mut().click.mode = mode.label().to_ascii_lowercase();
                *this.config_dirty.borrow_mut() = true;
            }
        });

        window.burst_row.connect_notify_local(Some("value"), {
            let this = Rc::clone(&this);
            move |row, _| {
                let count = row.value() as u32;
                this.engine
                    .borrow()
                    .send(EngineCommand::SetBurstCount(count));
                this.config.borrow_mut().click.burst_count = count;
                *this.config_dirty.borrow_mut() = true;
            }
        });

        window.button_row.connect_selected_notify({
            let this = Rc::clone(&this);
            move |row| {
                let button = if row.selected() == 1 {
                    MouseButton::Right
                } else {
                    MouseButton::Left
                };
                this.engine.borrow().send(EngineCommand::SetButton(button));
                this.config.borrow_mut().click.button = button.as_config_str().to_string();
                *this.config_dirty.borrow_mut() = true;
            }
        });

        // Hotkey capture: use channel to send result from bg thread back to main thread.
        // Clone widgets that we need before creating closures, since gtk widgets are
        // reference-counted and implementing Clone.
        let hk_btn = window.hotkey_btn.clone();
        let hk_label = window.hotkey_label.clone();
        let toast = window.toast_overlay.clone();

        let capture_result_rx: CaptureResultRx = Rc::new(RefCell::new(None));

        hk_btn.connect_clicked({
            let this = Rc::clone(&this);
            let hk_label = hk_label.clone();
            let hk_btn = hk_btn.clone();
            let toast = toast.clone();
            let capture_result_rx = Rc::clone(&capture_result_rx);
            move |_| {
                let mut cap_shutdown = this.capture_shutdown.borrow_mut();

                if let Some(shutdown) = cap_shutdown.as_ref() {
                    // Already capturing - cancel it
                    shutdown.store(true, Ordering::SeqCst);
                    cap_shutdown.take();
                    *capture_result_rx.borrow_mut() = None;
                    hk_btn.set_label("Set Hotkey");
                    hk_btn.remove_css_class("destructive-action");
                    hk_label.add_css_class("dim-label");
                    hk_label.remove_css_class("accent");
                    let t = libadwaita::Toast::new("Hotkey capture cancelled");
                    t.set_timeout(2);
                    toast.add_toast(t);
                } else {
                    // Start capturing
                    let shutdown = Arc::new(AtomicBool::new(false));
                    *cap_shutdown = Some(Arc::clone(&shutdown));

                    hk_btn.set_label("Cancel");
                    hk_btn.add_css_class("destructive-action");
                    hk_label.set_text("Press a key...");
                    hk_label.remove_css_class("dim-label");
                    hk_label.add_css_class("accent");

                    let (tx, rx) = crossbeam_channel::bounded(1);
                    *capture_result_rx.borrow_mut() = Some(rx);

                    let _ = thread::Builder::new()
                        .name("hotkey-capture".into())
                        .spawn(move || {
                            let result = hotkey::capture_next_key(shutdown);
                            let _ = tx.send(result);
                        });
                }
            }
        });

        // Poll for capture results from the main thread
        let this_poll = Rc::clone(&this);
        let cap_rx = Rc::clone(&capture_result_rx);
        glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
            if let Some(rx) = cap_rx.borrow_mut().as_ref()
                && let Ok(result) = rx.try_recv()
            {
                let hk_label = hk_label.clone();
                let hk_btn = hk_btn.clone();
                let toast = toast.clone();

                match result {
                    Ok(key_code) => {
                        if !hotkey::is_safe_key(key_code) {
                            let name = hotkey::key_code_to_name(key_code);
                            hk_label.set_text(&name);
                            hk_btn.set_label("Set Hotkey");
                            hk_btn.remove_css_class("destructive-action");
                            hk_label.add_css_class("dim-label");
                            hk_label.remove_css_class("accent");
                            let t = libadwaita::Toast::new(&format!(
                                "{name} is not allowed as a hotkey"
                            ));
                            t.set_timeout(2);
                            toast.add_toast(t);
                            *this_poll.capture_shutdown.borrow_mut() = None;
                            return glib::ControlFlow::Continue;
                        }

                        let key_name = hotkey::key_code_to_name(key_code);
                        hk_label.set_text(&key_name);
                        hk_btn.set_label("Set Hotkey");
                        hk_btn.remove_css_class("destructive-action");
                        hk_label.add_css_class("dim-label");
                        hk_label.remove_css_class("accent");
                        let t = libadwaita::Toast::new(&format!("Hotkey set to {key_name}"));
                        t.set_timeout(2);
                        toast.add_toast(t);

                        // Save to config
                        this_poll.config.borrow_mut().hotkey.key = key_name.clone();
                        *this_poll.config_dirty.borrow_mut() = true;

                        // Restart hotkey listener with new key
                        this_poll.restart_hotkey(&key_name);

                        *this_poll.capture_shutdown.borrow_mut() = None;
                    }
                    Err(_) => {
                        // Capture was cancelled or failed
                        hk_btn.set_label("Set Hotkey");
                        hk_btn.remove_css_class("destructive-action");
                        hk_label.add_css_class("dim-label");
                        hk_label.remove_css_class("accent");
                        *this_poll.capture_shutdown.borrow_mut() = None;
                    }
                }
            }
            glib::ControlFlow::Continue
        });

        window.jitter_switch.connect_active_notify({
            let this = Rc::clone(&this);
            move |sw| {
                let enabled = sw.is_active();
                this.engine.borrow().send(EngineCommand::SetJitter(enabled));
                this.config.borrow_mut().click.jitter_enabled = enabled;
                *this.config_dirty.borrow_mut() = true;
            }
        });
    }

    fn start_hotkey(self: &Rc<Self>, key: &str) {
        let mut hotkey = self.hotkey.borrow_mut();
        let _ = hotkey.set_key(key);
        match hotkey.start() {
            Ok(()) => {
                tracing::info!("hotkey listener started for key {key}");
                let rx = hotkey.event_receiver();
                self.engine.borrow().send(EngineCommand::AttachHotkey(rx));
            }
            Err(e) => {
                tracing::error!("failed to start hotkey listener: {e}");
            }
        }
    }

    fn restart_hotkey(self: &Rc<Self>, key: &str) {
        let mut hotkey = self.hotkey.borrow_mut();
        hotkey.stop();
        let _ = hotkey.set_key(key);
        match hotkey.start() {
            Ok(()) => {
                tracing::info!("hotkey listener restarted for key {key}");
                let rx = hotkey.event_receiver();
                self.engine.borrow().send(EngineCommand::AttachHotkey(rx));
            }
            Err(e) => {
                tracing::error!("failed to restart hotkey listener: {e}");
            }
        }
    }

    fn start_stats_poll(self: &Rc<Self>, window: &MainWindow) {
        let this = Rc::clone(self);
        let live_cps_label = window.live_cps_label.clone();
        let total_clicks_label = window.total_clicks_label.clone();
        let status_label = window.status_label.clone();
        let start_stop_btn = window.start_stop_btn.clone();
        let status_icon = window.status_icon.clone();
        let toast_overlay = window.toast_overlay.clone();
        let last_running = Rc::new(RefCell::new(false));
        let last_device_error: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));

        glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
            if let Some(stats) = this.engine.borrow().try_recv_stats() {
                let is_running = stats.running;
                let was_running = *last_running.borrow();

                live_cps_label.set_text(&format!("Live: {:.1} CPS", stats.live_cps));
                total_clicks_label.set_text(&format!("Clicks: {}", stats.total_clicks));

                if stats.device_error != *last_device_error.borrow() {
                    if let Some(ref err) = stats.device_error {
                        let t = libadwaita::Toast::new(&format!("Device error: {err}"));
                        t.set_timeout(4);
                        toast_overlay.add_toast(t);
                    }
                    *last_device_error.borrow_mut() = stats.device_error.clone();
                }

                // Only update UI state when it actually changes
                if is_running != was_running {
                    if is_running {
                        status_label.set_text("Running");
                        status_label.remove_css_class("dim-label");
                        status_icon.remove_css_class("error");
                        status_icon.add_css_class("success");
                        start_stop_btn.set_label("Stop Clicking");
                        start_stop_btn.remove_css_class("suggested-action");
                        start_stop_btn.add_css_class("destructive-action");
                    } else {
                        status_label.set_text("Stopped");
                        status_label.add_css_class("dim-label");
                        status_icon.remove_css_class("success");
                        status_icon.add_css_class("error");
                        start_stop_btn.set_label("Start Clicking");
                        start_stop_btn.remove_css_class("destructive-action");
                        start_stop_btn.add_css_class("suggested-action");
                    }
                    *last_running.borrow_mut() = is_running;
                }

                *this.running.borrow_mut() = is_running;
            }
            glib::ControlFlow::Continue
        });
    }
}

pub fn mode_from_config(mode: &str) -> ClickMode {
    match mode.trim().to_ascii_lowercase().as_str() {
        "hold" => ClickMode::Hold,
        "burst" => ClickMode::Burst,
        _ => ClickMode::Toggle,
    }
}

pub fn engine_settings_from_config(config: &AppConfig) -> EngineSettings {
    EngineSettings {
        cps: config.cps.default,
        mode: mode_from_config(&config.click.mode),
        button: MouseButton::from_config(&config.click.button),
        burst_count: config.click.burst_count,
        jitter_enabled: config.click.jitter_enabled,
    }
}
