# Clix - Linux Auto Clicker

A modern, production-grade auto-clicker for Linux with a beautiful GTK4/libadwaita interface. Injects mouse clicks at the kernel level via **uinput**, making it compatible with Wayland and X11 without requiring desktop portals.

![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)

## ⚠️ Security & Usage Disclaimer

**IMPORTANT: Please read before use**

- **Gaming**: Many online games and competitive platforms prohibit the use of auto-clickers and consider them cheating tools. Using this software may result in account suspension or permanent bans. Always check the terms of service before using.
- **Kernel-Level Access**: This software requires access to `/dev/uinput` for mouse input injection and keyboard event devices for hotkey detection. These are privileged operations that should be used responsibly.
- **Anti-Cheat Systems**: Some anti-cheat software may detect and flag this application as malicious.
- **Intended Use**: This tool is designed for legitimate automation tasks, testing, accessibility purposes, and personal productivity - not for gaining unfair advantages in games or violating platform policies.

**Use this software at your own risk. The authors are not responsible for any consequences resulting from its use.**

## Features

- 🎨 **Modern GTK4/libadwaita UI** - Beautiful, native Linux interface that follows system theme
- ⚡ **High Performance** - Precise timing with configurable CPS (1-1000 clicks per second)
- 🎮 **Multiple Modes**:
  - **Toggle**: Start/stop with hotkey press
  - **Hold**: Click while holding hotkey
  - **Burst**: Fire a fixed number of clicks
- 🖱️ **Left & Right Click** support
- ⌨️ **Global Hotkeys** - Works even when window is minimized (default: F6)
- 🎲 **Random Jitter** - Add randomness to click timing for more human-like behavior
- 🔧 **Persistent Configuration** - Settings saved automatically
- 🌊 **Wayland Compatible** - Works on modern Wayland compositors (Hyprland, Niri, KDE, GNOME)
- 💻 **Headless Mode** - Run without GUI for automation scripts

## Requirements

### Arch Linux / CachyOS / Manjaro

```bash
sudo pacman -S rust gtk4 libadwaita pkg-config base-devel
```

### Ubuntu / Debian / Pop!_OS

```bash
sudo apt install build-essential cargo libgtk-4-dev libadwaita-1-dev pkg-config
```

### Fedora

```bash
sudo dnf install rust cargo gtk4-devel libadwaita-devel pkg-config gcc
```

### openSUSE

```bash
sudo zypper install rust cargo gtk4-devel libadwaita-devel pkg-config gcc
```

## Build & Installation

### 1. Clone the Repository

```bash
git clone https://github.com/yourusername/clix.git
cd clix
```

### 2. Build from Source

```bash
# Debug build (faster compilation, slower runtime)
cargo build

# Release build (optimized, recommended for actual use)
cargo build --release
```

### 3. Install System-Wide (Optional)

```bash
sudo cp target/release/clix /usr/local/bin/clix
```

Or just run directly:
```bash
./target/release/clix
```

## Setting Up Permissions

This application requires special permissions to inject mouse clicks and detect global hotkeys.

### Method 1: udev Rules (Recommended)

This grants access to uinput and input devices for your user without requiring root:

```bash
# Copy the udev rule
sudo cp udev/99-clix-uinput.rules /etc/udev/rules.d/

# Reload udev rules
sudo udevadm control --reload-rules
sudo udevadm trigger

# Add your user to the input group
sudo usermod -aG input $USER
```

**Important**: You must log out and log back in (or reboot) for group membership changes to take effect.

### Method 2: Manual Permissions (Temporary)

For testing purposes only:

```bash
sudo chmod 666 /dev/uinput
```

Note: This is reset on reboot and is less secure.

### Verifying Permissions

Check if you have proper access:

```bash
# Check uinput access
ls -l /dev/uinput

# Check group membership
groups | grep input

# Check event devices
ls -l /dev/input/event*
```

If permissions are correct, the app will show "Backend: active (uinput)" when running.

## Usage

### Basic Usage

```bash
# Run with GUI
./target/release/clix

# Or if installed system-wide
clix
```

### Command Line Options

| Flag | Description |
|------|-------------|
| `--debug` | Enable verbose debug logging |
| `--no-gui` | Run in headless mode (for testing/automation) |

### GUI Controls

1. **Clicks Per Second (CPS)**: Adjust the clicking speed (1-1000 CPS)
   - Quick presets: 10, 20, 50, 100 CPS buttons
2. **Mode**: 
   - **Toggle**: Click hotkey to start/stop clicking
   - **Hold**: Clicks while hotkey is held down
   - **Burst**: Performs a fixed number of clicks then stops
3. **Mouse Button**: Choose left or right click
4. **Hotkey**: Customize your hotkey (default: F6)
   - Allowed keys: F1-F12, SPACE, TAB, ESC
   - System keys (CTRL, ALT, etc.) are blocked for safety
5. **Random Jitter**: Add randomness to click intervals
   - Configure min/max jitter in milliseconds

### Configuration File

Settings are automatically saved to:
```
~/.config/clix/config.toml
```

Example configuration:
```toml
[cps]
default = 20

[click]
button = "left"
jitter_enabled = false
jitter_min_ms = 5
jitter_max_ms = 20
burst_count = 10

[hotkey]
key = "F6"
```

### Optional: Systemd User Service

Run Clix automatically on login:

```bash
# Copy service file
mkdir -p ~/.config/systemd/user
cp systemd/clix.service ~/.config/systemd/user/

# Enable and start
systemctl --user daemon-reload
systemctl --user enable --now clix.service

# Check status
systemctl --user status clix.service
```

## Architecture

```
src/
├── backend/          # uinput virtual mouse driver
│   ├── input_emulation.rs
│   └── uinput_driver.rs
├── core/             # Click engine core logic
│   ├── click_engine.rs
│   ├── scheduler.rs
│   └── timing.rs
├── system/           # System integration
│   ├── hotkey.rs      # Global hotkey listener
│   └── permissions.rs # Permission checking
├── ui/               # GTK4/libadwaita interface
│   ├── controller.rs  # UI event handling
│   └── window.rs      # Window layout
└── utils/            # Utilities
    ├── config.rs      # Configuration management
    └── logging.rs     # Logging setup
```

## Wayland Compatibility

Unlike many auto-clickers that rely on X11-specific tools (xdotool, xautomation), Clix works at the kernel input layer using uinput. This makes it fully compatible with:

- ✅ Wayland compositors (Hyprland, Niri, Sway, etc.)
- ✅ X11 window managers
- ✅ KDE Plasma (X11 and Wayland)
- ✅ GNOME (X11 and Wayland)
- ✅ No need for RemoteDesktop or InputCapture portals

## Troubleshooting

### "Failed to open /dev/uinput"

- Make sure you've installed the udev rules
- Verify you're in the `input` group: `groups | grep input`
- Log out and back in after adding yourself to the group
- Check permissions: `ls -l /dev/uinput`

### Hotkey Not Working

- Ensure you're in the `input` group
- Check that `/dev/input/event*` devices are readable
- Some full-screen applications grab exclusive keyboard access
- Try a different hotkey (F6-F12 usually work best)

### Application Won't Start

```bash
# Run with debug output
./target/release/clix --debug

# Check dependencies
ldd target/release/clix
```

### Clicks Not Registering

- Verify "Backend: active (uinput)" shows in status
- Some applications ignore uinput events (security feature)
- Try in a different application (e.g., text editor) to verify it works

## Known Limitations

- **Input Group Required**: Global hotkeys require membership in the `input` group
- **Exclusive Grabs**: Some full-screen applications (games) use exclusive keyboard grabs that may block hotkey detection
- **Anti-Cheat**: May be detected by anti-cheat software in games
- **Game Compatibility**: Some games ignore uinput events as a security measure
- **Root Not Recommended**: Running as root is unnecessary and potentially dangerous

## Contributing

Contributions are welcome! Please feel free to submit pull requests or open issues for bugs and feature requests.

## Development

### Running Tests

```bash
# Run all tests
cargo test

# Run with output
cargo test -- --nocapture

# Run specific test
cargo test test_timing
```

### Debug Build

```bash
cargo build
./target/debug/clix --debug
```

## License

MIT License - See LICENSE file for details

## Credits

Built with:
- [Rust](https://www.rust-lang.org/)
- [GTK4](https://www.gtk.org/)
- [libadwaita](https://gnome.pages.gitlab.gnome.org/libadwaita/)
- [evdev](https://crates.io/crates/evdev) for hotkey detection
- [uinput](https://www.kernel.org/doc/html/latest/input/uinput.html) for mouse injection

## Support

If you encounter issues:
1. Check the Troubleshooting section above
2. Run with `--debug` flag for detailed logs
3. Open an issue on GitHub with debug output

---

**Remember**: Use responsibly and in accordance with applicable terms of service.
