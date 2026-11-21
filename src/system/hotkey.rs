use std::fs;
use std::io;
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HotkeyError {
    #[error("unsupported hotkey: {0}")]
    UnsupportedKey(String),
    #[error("no keyboard event devices found in /dev/input")]
    NoDevices,
    #[error("cannot access /dev/input: {0}")]
    Io(#[from] io::Error),
    #[error("hotkey capture cancelled")]
    Cancelled,
    #[error("failed to spawn hotkey thread: {0}")]
    ThreadSpawn(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyAction {
    Press,
    Release,
}

#[derive(Debug, Clone)]
pub struct HotkeyEvent {
    pub action: HotkeyAction,
}

pub struct HotkeyListener {
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    event_tx: Sender<HotkeyEvent>,
    event_rx: Receiver<HotkeyEvent>,
    key_code: u16,
}

impl HotkeyListener {
    pub fn new(key_name: &str) -> Result<Self, HotkeyError> {
        let key_code = parse_key_code(key_name)?;
        let (event_tx, event_rx) = crossbeam_channel::unbounded();
        let shutdown = Arc::new(AtomicBool::new(false));

        Ok(Self {
            shutdown,
            handle: None,
            event_tx,
            event_rx,
            key_code,
        })
    }

    pub fn event_receiver(&self) -> Receiver<HotkeyEvent> {
        self.event_rx.clone()
    }

    pub fn set_key(&mut self, key_name: &str) -> Result<(), HotkeyError> {
        self.key_code = parse_key_code(key_name)?;
        Ok(())
    }

    pub fn start(&mut self) -> Result<(), HotkeyError> {
        if self.handle.is_some() {
            return Ok(());
        }

        let event_files = discover_event_devices()?;
        if event_files.is_empty() {
            return Err(HotkeyError::NoDevices);
        }

        let shutdown = Arc::clone(&self.shutdown);
        let event_tx = self.event_tx.clone();
        let key_code = self.key_code;

        let handle = thread::Builder::new()
            .name("hotkey-listener".into())
            .spawn(move || hotkey_loop(event_files, key_code, event_tx, shutdown))
            .map_err(|e| HotkeyError::ThreadSpawn(e.to_string()))?;

        self.handle = Some(handle);
        Ok(())
    }

    pub fn stop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        self.shutdown = Arc::new(AtomicBool::new(false));
    }
}

impl Drop for HotkeyListener {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Capture the next key pressed on any input device.
/// Returns the key code (number) of whatever key is pressed next.
/// Can be cancelled via the shutdown flag.
pub fn capture_next_key(shutdown: Arc<AtomicBool>) -> Result<u16, String> {
    capture_next_key_inner(shutdown).map_err(|e| e.to_string())
}

fn capture_next_key_inner(shutdown: Arc<AtomicBool>) -> Result<u16, HotkeyError> {
    let mut event_files = discover_event_devices()?;
    if event_files.is_empty() {
        return Err(HotkeyError::NoDevices);
    }

    set_nonblocking_and_drain(&mut event_files);
    let mut pollfds = make_pollfds(&event_files);
    let mut events = Vec::with_capacity(16);

    while !shutdown.load(Ordering::SeqCst) {
        events.clear();
        poll_ready_events(&mut pollfds, 50, &mut events);
        for raw in &events {
            // Only capture key presses (value == 1), ignore releases and repeats
            if raw.type_ == EV_KEY && raw.value == 1 && raw.code < 0x100 {
                tracing::info!("captured key code: {}", raw.code);
                return Ok(raw.code);
            }
        }
    }

    Err(HotkeyError::Cancelled)
}

const EV_KEY: u16 = 0x01;

/// Key codes for the hotkey
const KEY_F1: u16 = 59;
const KEY_F2: u16 = 60;
const KEY_F3: u16 = 61;
const KEY_F4: u16 = 62;
const KEY_F5: u16 = 63;
const KEY_F6: u16 = 64;
const KEY_F7: u16 = 65;
const KEY_F8: u16 = 66;
const KEY_F9: u16 = 67;
const KEY_F10: u16 = 68;
const KEY_F11: u16 = 87;
const KEY_F12: u16 = 88;
const KEY_SPACE: u16 = 57;
const KEY_TAB: u16 = 15;
const KEY_ESC: u16 = 1;
const KEY_ENTER: u16 = 28;
const KEY_BACKSPACE: u16 = 14;
const KEY_DELETE: u16 = 111;
const KEY_UP: u16 = 103;
const KEY_DOWN: u16 = 108;
const KEY_LEFT: u16 = 105;
const KEY_RIGHT: u16 = 106;
const KEY_HOME: u16 = 102;
const KEY_END: u16 = 107;
const KEY_PAGEUP: u16 = 104;
const KEY_PAGEDOWN: u16 = 109;
const KEY_INSERT: u16 = 110;
const KEY_SLASH: u16 = 53;
const KEY_DOT: u16 = 52;
const KEY_MINUS: u16 = 12;
const KEY_EQUAL: u16 = 13;
const KEY_LEFTBRACE: u16 = 26;
const KEY_RIGHTBRACE: u16 = 27;
const KEY_SEMICOLON: u16 = 39;
const KEY_APOSTROPHE: u16 = 40;
const KEY_GRAVE: u16 = 41;
const KEY_BACKSLASH: u16 = 43;
const KEY_COMMA: u16 = 51;

struct KeyMapping {
    code: u16,
    display: &'static str,
    aliases: &'static [&'static str],
}

const KEY_MAPPINGS: &[KeyMapping] = &[
    KeyMapping {
        code: KEY_F1,
        display: "F1",
        aliases: &["F1"],
    },
    KeyMapping {
        code: KEY_F2,
        display: "F2",
        aliases: &["F2"],
    },
    KeyMapping {
        code: KEY_F3,
        display: "F3",
        aliases: &["F3"],
    },
    KeyMapping {
        code: KEY_F4,
        display: "F4",
        aliases: &["F4"],
    },
    KeyMapping {
        code: KEY_F5,
        display: "F5",
        aliases: &["F5"],
    },
    KeyMapping {
        code: KEY_F6,
        display: "F6",
        aliases: &["F6"],
    },
    KeyMapping {
        code: KEY_F7,
        display: "F7",
        aliases: &["F7"],
    },
    KeyMapping {
        code: KEY_F8,
        display: "F8",
        aliases: &["F8"],
    },
    KeyMapping {
        code: KEY_F9,
        display: "F9",
        aliases: &["F9"],
    },
    KeyMapping {
        code: KEY_F10,
        display: "F10",
        aliases: &["F10"],
    },
    KeyMapping {
        code: KEY_F11,
        display: "F11",
        aliases: &["F11"],
    },
    KeyMapping {
        code: KEY_F12,
        display: "F12",
        aliases: &["F12"],
    },
    KeyMapping {
        code: KEY_SPACE,
        display: "Space",
        aliases: &["SPACE"],
    },
    KeyMapping {
        code: KEY_TAB,
        display: "Tab",
        aliases: &["TAB"],
    },
    KeyMapping {
        code: KEY_ESC,
        display: "Esc",
        aliases: &["ESC", "ESCAPE"],
    },
    KeyMapping {
        code: KEY_ENTER,
        display: "Enter",
        aliases: &["ENTER"],
    },
    KeyMapping {
        code: KEY_BACKSPACE,
        display: "Backspace",
        aliases: &["BACKSPACE"],
    },
    KeyMapping {
        code: KEY_DELETE,
        display: "Delete",
        aliases: &["DELETE"],
    },
    KeyMapping {
        code: KEY_UP,
        display: "Up Arrow",
        aliases: &["UP", "UP_ARROW", "UP ARROW"],
    },
    KeyMapping {
        code: KEY_DOWN,
        display: "Down Arrow",
        aliases: &["DOWN", "DOWN_ARROW", "DOWN ARROW"],
    },
    KeyMapping {
        code: KEY_LEFT,
        display: "Left Arrow",
        aliases: &["LEFT", "LEFT_ARROW", "LEFT ARROW"],
    },
    KeyMapping {
        code: KEY_RIGHT,
        display: "Right Arrow",
        aliases: &["RIGHT", "RIGHT_ARROW", "RIGHT ARROW"],
    },
    KeyMapping {
        code: KEY_HOME,
        display: "Home",
        aliases: &["HOME"],
    },
    KeyMapping {
        code: KEY_END,
        display: "End",
        aliases: &["END"],
    },
    KeyMapping {
        code: KEY_PAGEUP,
        display: "Page Up",
        aliases: &["PAGEUP", "PAGE UP"],
    },
    KeyMapping {
        code: KEY_PAGEDOWN,
        display: "Page Down",
        aliases: &["PAGEDOWN", "PAGE DOWN"],
    },
    KeyMapping {
        code: KEY_INSERT,
        display: "Insert",
        aliases: &["INSERT"],
    },
    KeyMapping {
        code: KEY_SLASH,
        display: "/",
        aliases: &["/"],
    },
    KeyMapping {
        code: KEY_DOT,
        display: ".",
        aliases: &["."],
    },
    KeyMapping {
        code: KEY_MINUS,
        display: "-",
        aliases: &["-"],
    },
    KeyMapping {
        code: KEY_EQUAL,
        display: "=",
        aliases: &["="],
    },
    KeyMapping {
        code: KEY_LEFTBRACE,
        display: "[",
        aliases: &["["],
    },
    KeyMapping {
        code: KEY_RIGHTBRACE,
        display: "]",
        aliases: &["]"],
    },
    KeyMapping {
        code: KEY_SEMICOLON,
        display: ";",
        aliases: &[";"],
    },
    KeyMapping {
        code: KEY_APOSTROPHE,
        display: "'",
        aliases: &["'"],
    },
    KeyMapping {
        code: KEY_GRAVE,
        display: "`",
        aliases: &["`"],
    },
    KeyMapping {
        code: KEY_BACKSLASH,
        display: "\\",
        aliases: &["\\"],
    },
    KeyMapping {
        code: KEY_COMMA,
        display: ",",
        aliases: &[","],
    },
];

/// Convert a numeric key code back to a human-readable name
pub fn key_code_to_name(code: u16) -> String {
    if let Some(m) = KEY_MAPPINGS.iter().find(|m| m.code == code) {
        return m.display.to_string();
    }
    format!("Key({code})")
}

/// Check if a key code is safe (not a system-critical key)
pub fn is_safe_key(code: u16) -> bool {
    // System-critical keys that should not be used as hotkeys
    const UNSAFE_KEYS: &[u16] = &[
        KEY_ESC,
        KEY_ENTER,
        KEY_BACKSPACE,
        KEY_DELETE,
        KEY_TAB,
        // Modifier keys
        29,  // KEY_LEFTCTRL
        97,  // KEY_RIGHTCTRL
        56,  // KEY_LEFTALT
        100, // KEY_RIGHTALT
        42,  // KEY_LEFTSHIFT
        54,  // KEY_RIGHTSHIFT
        125, // KEY_LEFTMETA
        126, // KEY_RIGHTMETA
        // System keys
        127, // KEY_COMPOSE
        142, // KEY_SLEEP
        143, // KEY_WAKEUP
        124, // KEY_STOP
    ];
    !UNSAFE_KEYS.contains(&code)
}

pub fn parse_key_code(name: &str) -> Result<u16, HotkeyError> {
    let trimmed = name.trim();
    let upper = trimmed.to_uppercase();

    if let Some(m) = KEY_MAPPINGS
        .iter()
        .find(|m| m.aliases.iter().any(|&a| a == upper))
    {
        return Ok(m.code);
    }

    if let Some(inner) = upper.strip_prefix("KEY(").and_then(|s| s.strip_suffix(')'))
        && let Ok(code) = inner.trim().parse::<u16>()
        && code < 0x100
    {
        return Ok(code);
    }

    Err(HotkeyError::UnsupportedKey(name.to_string()))
}

fn has_keyboard_capability(file: &std::fs::File) -> bool {
    // EVIOCGBIT(0, 4): _IOC(_IOC_READ, 'E', 0x20, 4) = 0x80044520
    const EVIOCGBIT_0_4: libc::Ioctl = 0x80044520;
    // EVIOCGBIT(EV_KEY, 16): _IOC(_IOC_READ, 'E', 0x20 + 1, 16) = 0x80104521
    const EVIOCGBIT_KEY_16: libc::Ioctl = 0x80104521;

    let mut ev_bits: u32 = 0;
    let ret = unsafe { libc::ioctl(file.as_raw_fd(), EVIOCGBIT_0_4, &mut ev_bits as *mut u32) };
    if ret < 0 {
        return true;
    }
    if (ev_bits & (1 << EV_KEY)) == 0 {
        return false;
    }

    let mut key_bits = [0u8; 16];
    let ret = unsafe { libc::ioctl(file.as_raw_fd(), EVIOCGBIT_KEY_16, key_bits.as_mut_ptr()) };
    if ret < 0 {
        return true;
    }

    // Check for common keyboard keys (KEY_A=30, KEY_SPACE=57, KEY_F1=59) so mice/virtual mice are skipped.
    [30usize, 57, 59]
        .iter()
        .any(|&code| (key_bits[code / 8] & (1 << (code % 8))) != 0)
}

fn discover_event_devices() -> Result<Vec<std::fs::File>, HotkeyError> {
    let mut files = Vec::new();
    let mut unreadable = 0usize;
    let input_dir = Path::new("/dev/input");

    let entries = fs::read_dir(input_dir)?;

    for entry in entries.flatten() {
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if !name.starts_with("event") {
            continue;
        }

        if let Ok(file) = std::fs::OpenOptions::new().read(true).open(&path) {
            if has_keyboard_capability(&file) {
                tracing::info!("hotkey monitoring {path:?}");
                files.push(file);
            }
        } else {
            unreadable += 1;
            tracing::debug!("cannot open {path:?} (permissions?)");
        }
    }

    if files.is_empty() && unreadable > 0 {
        tracing::warn!(
            "could not open {unreadable} /dev/input/event* device(s) — ensure user is in the 'input' group"
        );
    }

    Ok(files)
}

fn count_event_nodes() -> usize {
    fs::read_dir("/dev/input")
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| {
                    e.file_name()
                        .to_str()
                        .is_some_and(|n| n.starts_with("event"))
                })
                .count()
        })
        .unwrap_or(0)
}

fn make_pollfds(event_files: &[std::fs::File]) -> Vec<libc::pollfd> {
    event_files
        .iter()
        .map(|file| libc::pollfd {
            fd: file.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        })
        .collect()
}

fn set_nonblocking_and_drain(event_files: &mut [std::fs::File]) {
    for file in event_files.iter_mut() {
        let fd = file.as_raw_fd();
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFL, 0);
            if flags >= 0 {
                libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
            }
            let mut buf = [0u8; 4096];
            loop {
                let ret = libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len());
                if ret <= 0 {
                    break;
                }
            }
        }
    }
}

fn poll_ready_events(
    pollfds: &mut [libc::pollfd],
    timeout_ms: libc::c_int,
    out: &mut Vec<libc::input_event>,
) {
    let ret = unsafe {
        libc::poll(
            pollfds.as_mut_ptr(),
            pollfds.len() as libc::nfds_t,
            timeout_ms,
        )
    };

    if ret < 0 {
        let err = io::Error::last_os_error();
        if err.kind() != io::ErrorKind::Interrupted {
            tracing::warn!("poll error: {err}");
        }
        return;
    }

    if ret == 0 {
        return;
    }

    let event_size = std::mem::size_of::<libc::input_event>();
    for pfd in pollfds.iter_mut() {
        if pfd.revents & libc::POLLIN == 0 {
            continue;
        }
        pfd.revents = 0;

        loop {
            let mut raw: libc::input_event = unsafe { std::mem::zeroed() };
            let buf_ptr = &mut raw as *mut libc::input_event as *mut libc::c_void;
            let n = unsafe { libc::read(pfd.fd, buf_ptr, event_size) };

            if n < 0 {
                let err = io::Error::last_os_error();
                if err.kind() == io::ErrorKind::WouldBlock {
                    break;
                }
                if err.kind() != io::ErrorKind::Interrupted {
                    tracing::warn!("read error on event device: {err}");
                    break;
                }
                continue;
            }

            if n == 0 {
                break;
            }

            if n as usize == event_size {
                out.push(raw);
            }
        }
    }
}

fn hotkey_loop(
    mut event_files: Vec<std::fs::File>,
    key_code: u16,
    event_tx: Sender<HotkeyEvent>,
    shutdown: Arc<AtomicBool>,
) {
    tracing::info!(
        "hotkey loop started with {} event device(s), watching for key code {}",
        event_files.len(),
        key_code
    );

    set_nonblocking_and_drain(&mut event_files);
    let mut pollfds = make_pollfds(&event_files);
    let mut events = Vec::with_capacity(16);
    let mut last_node_count = count_event_nodes();
    let mut last_rescan = Instant::now();

    while !shutdown.load(Ordering::SeqCst) {
        events.clear();
        poll_ready_events(&mut pollfds, 10, &mut events);
        for raw in &events {
            if raw.type_ == EV_KEY && raw.code == key_code {
                let action = match raw.value {
                    1 => HotkeyAction::Press,
                    0 => HotkeyAction::Release,
                    _ => continue,
                };
                let _ = event_tx.send(HotkeyEvent { action });
            }
        }

        if last_rescan.elapsed() >= Duration::from_secs(3) {
            last_rescan = Instant::now();
            let current_count = count_event_nodes();
            if current_count != last_node_count {
                last_node_count = current_count;
                if let Ok(mut updated) = discover_event_devices()
                    && !updated.is_empty()
                {
                    set_nonblocking_and_drain(&mut updated);
                    pollfds = make_pollfds(&updated);
                    event_files = updated;
                    tracing::info!(
                        "hotkey listener refreshed ({} device(s))",
                        event_files.len()
                    );
                }
            }
        }
    }

    tracing::info!("hotkey loop exiting");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_code_roundtrip_all_mapped_keys() {
        for m in KEY_MAPPINGS {
            let name = key_code_to_name(m.code);
            let parsed = parse_key_code(&name).unwrap_or_else(|e| {
                panic!("failed to parse '{}' for code {}: {}", name, m.code, e);
            });
            assert_eq!(parsed, m.code);
        }
    }

    #[test]
    fn test_generic_key_code_roundtrip() {
        let name = key_code_to_name(30); // KEY_A
        assert_eq!(name, "Key(30)");
        assert_eq!(parse_key_code(&name).unwrap(), 30);
    }

    #[test]
    fn test_is_safe_key_blocks_system_and_modifiers() {
        assert!(!is_safe_key(KEY_ESC));
        assert!(!is_safe_key(KEY_TAB));
        assert!(!is_safe_key(KEY_ENTER));
        assert!(!is_safe_key(29)); // KEY_LEFTCTRL
        assert!(is_safe_key(KEY_F6));
        assert!(is_safe_key(KEY_SPACE));
    }
}
