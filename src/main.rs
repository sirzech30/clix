use std::cell::RefCell;
use std::env;
use std::rc::Rc;

use gtk4::prelude::*;
use libadwaita::Application;
use libadwaita::prelude::*;

use clix::backend::MouseButton;
use clix::core::click_engine::ClickEngine;
use clix::core::scheduler::ClickMode;
use clix::system::hotkey::HotkeyListener;
use clix::system::permissions::check_permissions;
use clix::ui::controller::{engine_settings_from_config, mode_from_config};
use clix::ui::{AppController, MainWindow};
use clix::utils::config::AppConfig;
use clix::utils::logging::init_logging;

const APP_ID: &str = "com.clix.Pro";

fn print_help() {
    println!(
        "clix {} - Linux auto-clicker\n\n\
         USAGE:\n  clix [OPTIONS]\n\n\
         OPTIONS:\n  \
         --debug               Enable verbose debug logging\n  \
         --no-gui              Run a headless burst and exit\n  \
         --cps <N>             Clicks per second in --no-gui mode (1-1000)\n  \
         --count <N>           Burst click count in --no-gui mode (1-10000)\n  \
         --button <left|right> Mouse button in --no-gui mode\n  \
         -h, --help            Print help\n  \
         -V, --version         Print version",
        env!("CARGO_PKG_VERSION")
    );
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("clix {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    let debug = args.iter().any(|a| a == "--debug");
    let no_gui = args.iter().any(|a| a == "--no-gui");

    init_logging(debug);

    if no_gui {
        run_headless(&args);
        return;
    }

    libadwaita::init().expect("libadwaita init");

    let app = Application::builder().application_id(APP_ID).build();
    let controller_slot: Rc<RefCell<Option<Rc<AppController>>>> = Rc::new(RefCell::new(None));

    app.connect_activate({
        let controller_slot = Rc::clone(&controller_slot);
        move |app| {
            let config = AppConfig::load();
            let perms = check_permissions();

            let settings = engine_settings_from_config(&config);
            let engine = match ClickEngine::start(settings) {
                Ok(e) => e,
                Err(e) => {
                    show_startup_error(app, "Failed to start click engine", &e.to_string());
                    return;
                }
            };

            let hotkey = match HotkeyListener::new(&config.hotkey.key) {
                Ok(h) => h,
                Err(e) => {
                    show_startup_error(app, "Failed to initialize hotkey listener", &e.to_string());
                    return;
                }
            };

            let window = MainWindow::new(app, &perms);
            apply_config_to_window(&window, &config);
            let controller = AppController::new(&window, config, engine, hotkey);
            *controller_slot.borrow_mut() = Some(controller);
            window.window.present();
        }
    });

    app.connect_shutdown(move |_| {
        if let Some(controller) = controller_slot.borrow_mut().take() {
            controller.shutdown();
        }
    });

    // Pass empty slice so CLI flags like `--debug` aren't treated as file paths by GIO.
    let status = app.run_with_args::<&str>(&[]);
    std::process::exit(status.into());
}

fn show_startup_error(app: &Application, title: &str, details: &str) {
    eprintln!("{title}: {details}");
    let status = libadwaita::StatusPage::builder()
        .icon_name("dialog-error-symbolic")
        .title(title)
        .description(details)
        .build();
    let window = gtk4::ApplicationWindow::builder()
        .application(app)
        .title("Clix")
        .default_width(420)
        .default_height(320)
        .child(&status)
        .build();
    window.present();
}

fn apply_config_to_window(window: &MainWindow, config: &AppConfig) {
    window.cps_spin.set_value(config.cps.default as f64);
    window.set_cps_display(config.cps.default);
    window.set_hotkey_display(&config.hotkey.key);

    let mode = mode_from_config(&config.click.mode);
    window.mode_row.set_selected(mode.to_index());
    window.burst_row.set_visible(mode == ClickMode::Burst);

    let button_idx = if config.click.button.eq_ignore_ascii_case("right") {
        1
    } else {
        0
    };
    window.button_row.set_selected(button_idx);

    window.jitter_switch.set_active(config.click.jitter_enabled);
    window.burst_row.set_value(config.click.burst_count as f64);
}

fn parse_flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|w| w[0] == flag)
        .map(|w| w[1].as_str())
}

fn run_headless(args: &[String]) {
    use std::thread;
    use std::time::{Duration, Instant};

    use clix::core::click_engine::{ClickEngine, EngineCommand};

    let config = AppConfig::load();
    let mut settings = engine_settings_from_config(&config);
    settings.mode = ClickMode::Burst;
    settings.burst_count = parse_flag_value(args, "--count")
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(3)
        .clamp(1, 10_000);
    if let Some(cps) = parse_flag_value(args, "--cps").and_then(|v| v.parse::<u32>().ok()) {
        settings.cps = cps.clamp(1, 1000);
    }
    if let Some(btn) = parse_flag_value(args, "--button") {
        settings.button = MouseButton::from_config(btn);
    }

    let mut engine = ClickEngine::start(settings).expect("engine");
    engine.send(EngineCommand::Start);

    let deadline = Instant::now() + Duration::from_secs(30);
    let mut last_stats = None;
    let mut saw_running = false;

    while Instant::now() < deadline {
        if let Some(stats) = engine.try_recv_stats() {
            if stats.running {
                saw_running = true;
            }
            let done = !stats.running && (saw_running || !stats.device_active);
            last_stats = Some(stats);
            if done {
                break;
            }
        }
        thread::sleep(Duration::from_millis(20));
    }

    engine.shutdown();

    if let Some(stats) = last_stats {
        println!(
            "headless: clicks={} device_active={}",
            stats.total_clicks, stats.device_active
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_flag_value() {
        let args: Vec<String> = vec![
            "clix".into(),
            "--no-gui".into(),
            "--cps".into(),
            "50".into(),
            "--count".into(),
            "10".into(),
            "--button".into(),
            "right".into(),
        ];
        assert_eq!(parse_flag_value(&args, "--cps"), Some("50"));
        assert_eq!(parse_flag_value(&args, "--count"), Some("10"));
        assert_eq!(parse_flag_value(&args, "--button"), Some("right"));
        assert_eq!(parse_flag_value(&args, "--missing"), None);
    }
}
