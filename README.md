# Clix

Linux auto-clicker with a GTK4/libadwaita GUI. Injects mouse clicks at the kernel level via `/dev/uinput` and listens for global hotkeys on `/dev/input/event*`, so it works on both Wayland and X11 without desktop portals.

License: MIT

## Disclaimer

`clix` requires access to `/dev/uinput` and `/dev/input/event*` devices. Many online games and anti-cheat systems prohibit auto-clickers; use this tool only for legitimate automation, testing, and accessibility tasks.

## Features

- **Speed**: 1–1000 CPS with quick presets (`10`, `20`, `50`, `100`)
- **Modes**: `Toggle`, `Hold`, and `Burst` (configurable click count)
- **Mouse buttons**: Left and right click
- **Random jitter**: Optional ±15% random timing variation between clicks
- **Global hotkeys**: Works while unfocused or minimized (default `F6`), with interactive key capture
- **Persistent config**: Saves settings to `~/.config/clix/config.toml`
- **Headless mode**: Run a burst from the CLI with `--no-gui`

## Requirements

- **Arch / CachyOS / Manjaro**: `sudo pacman -S rust gtk4 libadwaita pkg-config base-devel`
- **Ubuntu / Debian / Pop!_OS**: `sudo apt install build-essential cargo libgtk-4-dev libadwaita-1-dev pkg-config`
- **Fedora**: `sudo dnf install rust cargo gtk4-devel libadwaita-devel pkg-config gcc`
- **openSUSE**: `sudo zypper install rust cargo gtk4-devel libadwaita-devel pkg-config gcc`

## Build

```bash
git clone https://github.com/sirzech30/clix.git
cd clix
cargo build --release
```

Run directly:

```bash
./target/release/clix
```

Or install system-wide:

```bash
sudo cp target/release/clix /usr/local/bin/clix
```

## Permissions Setup

`clix` needs read/write access to `/dev/uinput` and read access to keyboard `/dev/input/event*` devices.

### udev Rules (Recommended)

```bash
# Load uinput now and on boot
sudo modprobe uinput
sudo cp udev/clix-uinput.conf /etc/modules-load.d/

# Install and apply the udev rule
sudo cp udev/99-clix-uinput.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules
sudo udevadm trigger /dev/uinput

# Add your user to the input group
sudo usermod -aG input $USER
```

Log out and back in (or reboot) for group membership to take effect.

Verify:

```bash
groups | grep input
ls -l /dev/uinput
```

`/dev/uinput` should show `crw-rw----` owned by group `input`.

### Temporary Permissions (Testing Only)

```bash
sudo setfacl -m u:$USER:rw /dev/uinput
```

This resets on reboot.

## Usage

### GUI

```bash
cargo run --release
```

### CLI Options

| Flag | Description |
|---|---|
| `--debug` | Enable debug logging (`RUST_LOG` also supported) |
| `--no-gui` | Run a headless burst and exit |
| `--cps <N>` | Clicks per second in `--no-gui` mode (1–1000) |
| `--count <N>` | Burst click count in `--no-gui` mode (1–10000) |
| `--button <left\|right>` | Mouse button in `--no-gui` mode |
| `-h`, `--help` | Print help |
| `-V`, `--version` | Print version |

### Configuration

Settings are saved to `~/.config/clix/config.toml`:

```toml
[cps]
default = 20

[click]
mode = "toggle"
button = "left"
jitter_enabled = false
burst_count = 10

[hotkey]
key = "F6"
```

### Systemd User Service (Optional)

```bash
mkdir -p ~/.config/systemd/user
cp systemd/clix.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now clix.service
```

## Architecture

```text
src/
├── backend/   # /dev/uinput virtual mouse driver and ClickDevice trait
├── core/      # Click engine thread, mode scheduler, and timing/jitter
├── system/    # /dev/input/event* hotkey listener and permission checks
├── ui/        # GTK4/libadwaita window and controller
└── utils/     # TOML config validation/persistence and tracing setup
```

## Development

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## License

MIT — see [LICENSE](LICENSE).
