# Changelog

All notable changes to this project will be documented in this file.

## [0.1.1]

### Fixed
- Replaced min/max jitter inputs with a single Random Jitter toggle (±15% proportional variance).
- Prevented runtime panic in `apply_jitter` and `ClickEngine` on small/sub-millisecond intervals.
- Aligned hotkey capture names with `parse_key_code` so captured keys persist across restarts.
- Fixed supplementary `input` group detection in `permissions.rs` using `nix::unistd::getgroups()` and username matching.
- Removed 128 CPS ceiling in `compute_live_cps` by switching `click_window` to a time-pruned `VecDeque<Instant>`.
- Fixed `Cargo.toml` Debian icon asset path (`icons/clix.svg`) and `README.md` documentation drift.
- Resolved all `cargo clippy --all-targets` warnings.

### Added
- Persisted `click.mode` (`toggle`, `hold`, `burst`) in `config.toml`.
- Surface `ClickEngine` device errors in the GUI via `ToastOverlay` and disable jitter min/max inputs when jitter is off.
- Added `RUST_LOG` environment filter support in `init_logging`.
- Added unit tests for `ClickEngine` (via `ClickDevice` trait), `hotkey` round-trip/safety, `permissions`, `config` mode persistence, and CLI flag parsing.
- Added `--help`, `--version`, and configurable `--no-gui` (`--cps`, `--count`, `--button`) CLI options; fixed GIO file-open error on CLI flags via `run_with_args`.
- Added deterministic `AppController::shutdown` on `GApplication` shutdown.
- Added keyboard capability filtering (`EVIOCGBIT`), full-queue event draining, and keyboard hotplug rescanning in `hotkey.rs` with typed `HotkeyError`.
- Added `desktop/clix.desktop`, `total_clicks` counter in the UI, and Wayland-compatible `PassEnvironment` in `systemd/clix.service`.
- Added GitHub Actions CI & release workflows, Dependabot config, `SECURITY.md`, `CONTRIBUTING.md`, and `.editorconfig`.

## [0.1.0]
- Initial release with GTK4/libadwaita UI, `/dev/uinput` mouse injection, and global hotkeys.
