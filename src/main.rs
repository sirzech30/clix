use std::env;

use gtk4::prelude::*;
use libadwaita::prelude::*;
use libadwaita::Application;

use clix::core::click_engine::ClickEngine;
use clix::system::hotkey::HotkeyListener;
use clix::system::permissions::check_permissions;
use clix::ui::{AppController, MainWindow};
use clix::utils::config::AppConfig;
use clix::utils::logging::init_logging;

use clix::ui::controller::engine_settings_from_config;

const APP_ID: &str = "com.clix.Pro";

fn main() {
    let args: Vec<String> = env::args().collect();
    let debug = args.iter().any(|a| a == "--debug");
    let no_gui = args.iter().any(|a| a == "--no-gui");

    init_logging(debug);

    if no_gui {
        run_headless();
        return;
    }

    libadwaita::init().expect("libadwaita init");

    let app = Application::builder().application_id(APP_ID).build();

    app.connect_activate(|app| {
        let config = AppConfig::load();
        let perms = check_permissions();

        let settings = engine_settings_from_config(&config);
        let engine = match ClickEngine::start(settings) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("Failed to start click engine: {e}");
                return;
            }
        };

        let hotkey = match HotkeyListener::new(&config.hotkey.key) {
            Ok(h) => h,
            Err(e) => {
                eprintln!("Failed to init hotkey listener: {e}");
                return;
            }
        };

        let window = MainWindow::new(app, &perms);
        apply_config_to_window(&window, &config);
        let _controller = AppController::new(&window, config, engine, hotkey);
        window.window.present();
    });

    let status = app.run();
    std::process::exit(status.into());
}

fn apply_config_to_window(window: &MainWindow, config: &AppConfig) {
    window.cps_spin.set_value(config.cps.default as f64);
    window.set_cps_display(config.cps.default);
    window.set_hotkey_display(&config.hotkey.key);

    let button_idx = if config.click.button.eq_ignore_ascii_case("right") {
        1
    } else {
        0
    };
    window.button_row.set_selected(button_idx);

    window.jitter_switch.set_active(config.click.jitter_enabled);
    window
        .jitter_min_row
        .set_value(config.click.jitter_min_ms as f64);
    window
        .jitter_max_row
        .set_value(config.click.jitter_max_ms as f64);
    window
        .burst_row
        .set_value(config.click.burst_count as f64);
}

fn run_headless() {
    use std::thread;
    use std::time::Duration;

    use clix::core::click_engine::{ClickEngine, EngineCommand};
    use clix::core::scheduler::ClickMode;

    let config = AppConfig::load();
    let mut settings = engine_settings_from_config(&config);
    settings.mode = ClickMode::Burst;
    settings.burst_count = 3;

    let engine = ClickEngine::start(settings).expect("engine");
    engine.send(EngineCommand::Start);

    thread::sleep(Duration::from_secs(2));

    if let Some(stats) = engine.try_recv_stats() {
        println!(
            "headless: clicks={} device_active={}",
            stats.total_clicks, stats.device_active
        );
    }
}
