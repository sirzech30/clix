use std::ffi::CString;
use std::fs::OpenOptions;
use std::io;
use std::os::unix::io::{AsRawFd, RawFd};
use std::path::Path;

use libc::{INPUT_PROP_POINTER, input_event, timeval};

const EV_KEY: u32 = 0x01;
const EV_REL: u32 = 0x02;
const EV_SYN: u32 = 0x00;
const SYN_REPORT: u16 = 0;
use thiserror::Error;

// uinput ioctl numbers (arch-independent)
const UI_DEV_CREATE: libc::Ioctl = 0x5501;
const UI_DEV_DESTROY: libc::Ioctl = 0x5502;
const UI_SET_EVBIT: libc::Ioctl = 0x40045564;
const UI_SET_KEYBIT: libc::Ioctl = 0x40045565;
const UI_SET_RELBIT: libc::Ioctl = 0x40045566;
const UI_SET_PROPBIT: libc::Ioctl = 0x4004556d;

const BTN_LEFT: u16 = 0x110;
const BTN_RIGHT: u16 = 0x111;
const REL_X: u16 = 0x00;
const REL_Y: u16 = 0x01;

#[repr(C)]
struct uinput_user_dev {
    name: [u8; 80],
    id: libc::input_id,
    ff_effects_max: u32,
    absmax: [i32; 64],
    absmin: [i32; 64],
    absfuzz: [i32; 64],
    absflat: [i32; 64],
}

#[derive(Debug, Error)]
pub enum UInputError {
    #[error("failed to open /dev/uinput: {0}")]
    Open(io::Error),
    #[error("ioctl failed: {0}")]
    Ioctl(io::Error),
    #[error("write failed: {0}")]
    Write(io::Error),
    #[error("device not active")]
    Inactive,
}

pub struct UInputDriver {
    fd: std::fs::File,
    active: bool,
    last_error: Option<String>,
}

impl UInputDriver {
    pub fn new() -> Result<Self, UInputError> {
        let mut driver = Self {
            fd: open_uinput()?,
            active: false,
            last_error: None,
        };
        driver.create_device()?;
        Ok(driver)
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub fn recreate(&mut self) -> Result<(), UInputError> {
        if self.active {
            let _ = unsafe { libc::ioctl(fd_raw(&self.fd), UI_DEV_DESTROY, 0) };
            self.active = false;
        }
        self.create_device()
    }

    fn create_device(&mut self) -> Result<(), UInputError> {
        let fd = fd_raw(&self.fd);

        ioctl_set_bit(fd, UI_SET_EVBIT, EV_KEY as u64)?;
        ioctl_set_bit(fd, UI_SET_EVBIT, EV_REL as u64)?;
        ioctl_set_bit(fd, UI_SET_EVBIT, EV_SYN as u64)?;

        ioctl_set_bit(fd, UI_SET_KEYBIT, BTN_LEFT as u64)?;
        ioctl_set_bit(fd, UI_SET_KEYBIT, BTN_RIGHT as u64)?;
        ioctl_set_bit(fd, UI_SET_RELBIT, REL_X as u64)?;
        ioctl_set_bit(fd, UI_SET_RELBIT, REL_Y as u64)?;
        ioctl_set_bit(fd, UI_SET_PROPBIT, INPUT_PROP_POINTER as u64)?;

        // Write uinput_user_dev struct before UI_DEV_CREATE
        let dev_name = CString::new("Clix Virtual Mouse").map_err(|_| {
            UInputError::Ioctl(io::Error::new(io::ErrorKind::InvalidInput, "CString error"))
        })?;
        let mut dev = uinput_user_dev {
            name: [0u8; 80],
            id: libc::input_id {
                bustype: 0x03, // BUS_USB
                vendor: 0x046d,
                product: 0xc077,
                version: 1,
            },
            ff_effects_max: 0,
            absmax: [0; 64],
            absmin: [0; 64],
            absfuzz: [0; 64],
            absflat: [0; 64],
        };
        let name_bytes = dev_name.as_bytes();
        let len = name_bytes.len().min(79);
        dev.name[..len].copy_from_slice(&name_bytes[..len]);

        let dev_bytes = unsafe {
            std::slice::from_raw_parts(
                &dev as *const uinput_user_dev as *const u8,
                std::mem::size_of::<uinput_user_dev>(),
            )
        };
        write_all_bytes(&self.fd, dev_bytes).map_err(UInputError::Write)?;

        // UI_DEV_CREATE takes no argument
        if unsafe { libc::ioctl(fd, UI_DEV_CREATE, 0) } < 0 {
            return Err(UInputError::Ioctl(io::Error::last_os_error()));
        }

        self.active = true;
        self.last_error = None;
        tracing::info!("uinput virtual mouse created");
        Ok(())
    }

    pub fn write_event(&mut self, event: input_event) -> Result<(), UInputError> {
        if !self.active {
            return Err(UInputError::Inactive);
        }

        let bytes = unsafe {
            std::slice::from_raw_parts(
                &event as *const input_event as *const u8,
                std::mem::size_of::<input_event>(),
            )
        };

        match write_all_bytes(&self.fd, bytes) {
            Ok(()) => Ok(()),
            Err(e) => {
                let msg = e.to_string();
                self.last_error = Some(msg);
                self.active = false;
                Err(UInputError::Write(e))
            }
        }
    }

    pub fn syn_report(&mut self) -> Result<(), UInputError> {
        self.write_event(make_event(EV_SYN as u16, SYN_REPORT, 0))
    }
}

impl Drop for UInputDriver {
    fn drop(&mut self) {
        if self.active {
            let _ = unsafe { libc::ioctl(fd_raw(&self.fd), UI_DEV_DESTROY, 0) };
            self.active = false;
            tracing::info!("uinput virtual mouse destroyed");
        }
    }
}

fn fd_raw(file: &std::fs::File) -> RawFd {
    file.as_raw_fd()
}

fn open_uinput() -> Result<std::fs::File, UInputError> {
    for path in ["/dev/uinput", "/dev/input/uinput"] {
        if Path::new(path).exists() {
            return OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .map_err(UInputError::Open);
        }
    }
    OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/uinput")
        .map_err(UInputError::Open)
}

fn ioctl_set_bit(fd: RawFd, request: libc::Ioctl, bit: libc::c_ulong) -> Result<(), UInputError> {
    if unsafe { libc::ioctl(fd, request, bit) } < 0 {
        return Err(UInputError::Ioctl(io::Error::last_os_error()));
    }
    Ok(())
}

fn write_all_bytes(fd: &std::fs::File, mut data: &[u8]) -> io::Result<()> {
    while !data.is_empty() {
        match nix::unistd::write(fd, data) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "failed to write whole buffer",
                ));
            }
            Ok(n) => data = &data[n..],
            Err(nix::errno::Errno::EINTR) => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

pub fn make_event(type_: u16, code: u16, value: i32) -> input_event {
    input_event {
        time: timeval {
            tv_sec: 0,
            tv_usec: 0,
        },
        type_,
        code,
        value,
    }
}

pub const KEY_BTN_LEFT: u16 = BTN_LEFT;
pub const KEY_BTN_RIGHT: u16 = BTN_RIGHT;

pub const EV_KEY_TYPE: u16 = EV_KEY as u16;
pub const EV_REL_TYPE: u16 = EV_REL as u16;
pub const REL_X_CODE: u16 = REL_X;
pub const REL_Y_CODE: u16 = REL_Y;
