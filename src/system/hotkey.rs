use std::fs;
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use crossbeam_channel::{Receiver, Sender};

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
    pub fn new(key_name: &str) -> Result<Self, String> {
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

    pub fn set_key(&mut self, key_name: &str) -> Result<(), String> {
        self.key_code = parse_key_code(key_name)?;
        Ok(())
    }

    pub fn start(&mut self) -> Result<(), String> {
        if self.handle.is_some() {
            return Ok(());
        }

        let event_files = discover_event_devices()?;
        if event_files.is_empty() {
            return Err("no event devices found in /dev/input".into());
        }

        let shutdown = Arc::clone(&self.shutdown);
        let event_tx = self.event_tx.clone();
        let key_code = self.key_code;

        let handle = thread::Builder::new()
            .name("hotkey-listener".into())
            .spawn(move || hotkey_loop(event_files, key_code, event_tx, shutdown))
            .map_err(|e| e.to_string())?;

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
    let event_files = discover_event_devices()?;
    if event_files.is_empty() {
        return Err("no event devices found in /dev/input".into());
    }

    let event_size = std::mem::size_of::<input_event>();

    let mut pollfds: Vec<libc::pollfd> = event_files
        .iter()
        .map(|file| libc::pollfd {
            fd: file.as_raw_fd(),
            events: libc::POLLIN as i16,
            revents: 0,
        })
        .collect();

    // Drain stale events first
    for file in event_files.iter() {
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
            if flags >= 0 {
                libc::fcntl(fd, libc::F_SETFL, flags & !libc::O_NONBLOCK);
            }
        }
    }

    while !shutdown.load(Ordering::SeqCst) {
        let ret = unsafe {
            libc::poll(
                pollfds.as_mut_ptr(),
                pollfds.len() as libc::nfds_t,
                50, // 50ms poll timeout
            )
        };

        if ret < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() != std::io::ErrorKind::Interrupted {
                tracing::warn!("capture poll error: {err}");
            }
            continue;
        }

        if ret == 0 {
            continue;
        }

        for pfd in pollfds.iter_mut() {
            if pfd.revents & libc::POLLIN as i16 == 0 {
                continue;
            }
            pfd.revents = 0;

            let fd = pfd.fd;
            let mut raw: input_event = unsafe { std::mem::zeroed() };
            let buf_ptr = &mut raw as *mut input_event as *mut libc::c_void;
            let n = unsafe { libc::read(fd, buf_ptr, event_size) };

            if n < 0 || n as usize != event_size {
                continue;
            }

            // Only capture key presses (value == 1), ignore releases and repeats
            if raw.type_ == EV_KEY && raw.value == 1 {
                // Ignore mouse buttons (BTN_LEFT=0x110, BTN_RIGHT=0x111, etc.)
                if raw.code < 0x100 {
                    tracing::info!("captured key code: {}", raw.code);
                    return Ok(raw.code);
                }
            }
        }
    }

    Err("capture cancelled".into())
}

// Raw input_event struct matching /usr/include/linux/input.h
#[repr(C)]
#[derive(Clone, Copy)]
struct input_event {
    time: libc::timeval,
    type_: u16,
    code: u16,
    value: i32,
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

/// Convert a numeric key code back to a human-readable name
pub fn key_code_to_name(code: u16) -> String {
    match code {
        KEY_F1 => "F1".into(),
        KEY_F2 => "F2".into(),
        KEY_F3 => "F3".into(),
        KEY_F4 => "F4".into(),
        KEY_F5 => "F5".into(),
        KEY_F6 => "F6".into(),
        KEY_F7 => "F7".into(),
        KEY_F8 => "F8".into(),
        KEY_F9 => "F9".into(),
        KEY_F10 => "F10".into(),
        KEY_F11 => "F11".into(),
        KEY_F12 => "F12".into(),
        KEY_SPACE => "Space".into(),
        KEY_TAB => "Tab".into(),
        KEY_ESC => "Esc".into(),
        KEY_ENTER => "Enter".into(),
        KEY_BACKSPACE => "Backspace".into(),
        KEY_DELETE => "Delete".into(),
        KEY_UP => "Up Arrow".into(),
        KEY_DOWN => "Down Arrow".into(),
        KEY_LEFT => "Left Arrow".into(),
        KEY_RIGHT => "Right Arrow".into(),
        KEY_HOME => "Home".into(),
        KEY_END => "End".into(),
        KEY_PAGEUP => "Page Up".into(),
        KEY_PAGEDOWN => "Page Down".into(),
        KEY_INSERT => "Insert".into(),
        KEY_SLASH => "/".into(),
        KEY_DOT => ".".into(),
        KEY_MINUS => "-".into(),
        KEY_EQUAL => "=".into(),
        KEY_LEFTBRACE => "[".into(),
        KEY_RIGHTBRACE => "]".into(),
        KEY_SEMICOLON => ";".into(),
        KEY_APOSTROPHE => "'".into(),
        KEY_GRAVE => "`".into(),
        KEY_BACKSLASH => "\\".into(),
        KEY_COMMA => ",".into(),
        _ => format!("Key({code})"),
    }
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

pub fn parse_key_code(name: &str) -> Result<u16, String> {
    let upper = name.trim().to_uppercase();
    match upper.as_str() {
        "F1" => Ok(KEY_F1),
        "F2" => Ok(KEY_F2),
        "F3" => Ok(KEY_F3),
        "F4" => Ok(KEY_F4),
        "F5" => Ok(KEY_F5),
        "F6" => Ok(KEY_F6),
        "F7" => Ok(KEY_F7),
        "F8" => Ok(KEY_F8),
        "F9" => Ok(KEY_F9),
        "F10" => Ok(KEY_F10),
        "F11" => Ok(KEY_F11),
        "F12" => Ok(KEY_F12),
        "SPACE" => Ok(KEY_SPACE),
        "TAB" => Ok(KEY_TAB),
        "ESC" | "ESCAPE" => Ok(KEY_ESC),
        "ENTER" => Ok(KEY_ENTER),
        "BACKSPACE" => Ok(KEY_BACKSPACE),
        "DELETE" => Ok(KEY_DELETE),
        "UP" | "UP_ARROW" => Ok(KEY_UP),
        "DOWN" | "DOWN_ARROW" => Ok(KEY_DOWN),
        "LEFT" | "LEFT_ARROW" => Ok(KEY_LEFT),
        "RIGHT" | "RIGHT_ARROW" => Ok(KEY_RIGHT),
        "HOME" => Ok(KEY_HOME),
        "END" => Ok(KEY_END),
        "PAGEUP" => Ok(KEY_PAGEUP),
        "PAGEDOWN" => Ok(KEY_PAGEDOWN),
        "INSERT" => Ok(KEY_INSERT),
        _ => Err(format!("unsupported hotkey: {name}")),
    }
}

fn discover_event_devices() -> Result<Vec<std::fs::File>, String> {
    let mut files = Vec::new();
    let input_dir = Path::new("/dev/input");

    let entries = fs::read_dir(input_dir).map_err(|e| format!("cannot read {input_dir:?}: {e}"))?;

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
            tracing::info!("hotkey monitoring {path:?}");
            files.push(file);
        } else {
            tracing::warn!("cannot open {path:?} (permissions?)");
        }
    }

    Ok(files)
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

    // Drain any stale data from the event files
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
            if flags >= 0 {
                libc::fcntl(fd, libc::F_SETFL, flags & !libc::O_NONBLOCK);
            }
        }
    }

    let event_size = std::mem::size_of::<input_event>();

    let mut pollfds: Vec<libc::pollfd> = event_files
        .iter()
        .map(|file| libc::pollfd {
            fd: file.as_raw_fd(),
            events: libc::POLLIN as i16,
            revents: 0,
        })
        .collect();

    while !shutdown.load(Ordering::SeqCst) {
        let ret = unsafe {
            libc::poll(
                pollfds.as_mut_ptr(),
                pollfds.len() as libc::nfds_t,
                10,
            )
        };

        if ret < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() != std::io::ErrorKind::Interrupted {
                tracing::warn!("poll error: {err}");
            }
            continue;
        }

        if ret == 0 {
            continue;
        }

        for pfd in pollfds.iter_mut() {
            if pfd.revents & libc::POLLIN as i16 == 0 {
                continue;
            }

            pfd.revents = 0;

            let fd = pfd.fd;
            let mut raw: input_event = unsafe { std::mem::zeroed() };
            let buf_ptr = &mut raw as *mut input_event as *mut libc::c_void;
            let n = unsafe { libc::read(fd, buf_ptr, event_size) };

            if n < 0 {
                let err = std::io::Error::last_os_error();
                if err.kind() != std::io::ErrorKind::Interrupted {
                    tracing::warn!("read error on event device: {err}");
                }
                continue;
            }

            if n as usize != event_size {
                continue;
            }

            if raw.type_ == EV_KEY && raw.code == key_code {
                let action = match raw.value {
                    1 => HotkeyAction::Press,
                    0 => HotkeyAction::Release,
                    _ => continue,
                };
                let _ = event_tx.send(HotkeyEvent { action });
            }
        }
    }

    tracing::info!("hotkey loop exiting");
}