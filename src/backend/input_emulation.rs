use std::time::{Duration, Instant};

use super::uinput_driver::{
    make_event, UInputDriver, UInputError, EV_KEY_TYPE, EV_REL_TYPE, KEY_BTN_LEFT, KEY_BTN_RIGHT,
    REL_X_CODE, REL_Y_CODE,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
}

impl MouseButton {
    pub fn ev_code(self) -> u16 {
        match self {
            MouseButton::Left => KEY_BTN_LEFT,
            MouseButton::Right => KEY_BTN_RIGHT,
        }
    }

    pub fn from_config(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "right" => MouseButton::Right,
            _ => MouseButton::Left,
        }
    }

    pub fn as_config_str(self) -> &'static str {
        match self {
            MouseButton::Left => "left",
            MouseButton::Right => "right",
        }
    }
}

pub struct VirtualMouse {
    driver: UInputDriver,
}

impl VirtualMouse {
    pub fn new() -> Result<Self, UInputError> {
        Ok(Self {
            driver: UInputDriver::new()?,
        })
    }

    pub fn is_active(&self) -> bool {
        self.driver.is_active()
    }

    pub fn last_error(&self) -> Option<&str> {
        self.driver.last_error()
    }

    pub fn recreate(&mut self) -> Result<(), UInputError> {
        self.driver.recreate()
    }

    pub fn click(&mut self, button: MouseButton) -> Result<(), UInputError> {
        let code = button.ev_code();

        self.driver.write_event(make_event(EV_KEY_TYPE, code, 1))?;
        self.driver.syn_report()?;

        // Busy-spin for exactly 100µs instead of thread::sleep(1ms).
        // thread::sleep on Linux has poor granularity (~4ms with default 250Hz tick),
        // which kills CPS at high rates. A micro-spin gives precise timing.
        let spin_deadline = Instant::now() + Duration::from_micros(100);
        while Instant::now() < spin_deadline {
            std::hint::spin_loop();
        }

        self.driver.write_event(make_event(EV_KEY_TYPE, code, 0))?;
        self.driver.syn_report()?;

        self.driver
            .write_event(make_event(EV_REL_TYPE, REL_X_CODE, 0))?;
        self.driver
            .write_event(make_event(EV_REL_TYPE, REL_Y_CODE, 0))?;
        self.driver.syn_report()?;

        Ok(())
    }
}
