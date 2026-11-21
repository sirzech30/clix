use crate::system::hotkey::HotkeyAction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickMode {
    Toggle,
    Hold,
    Burst,
}

impl ClickMode {
    pub fn from_index(index: u32) -> Self {
        match index {
            1 => ClickMode::Hold,
            2 => ClickMode::Burst,
            _ => ClickMode::Toggle,
        }
    }

    pub fn to_index(self) -> u32 {
        match self {
            ClickMode::Toggle => 0,
            ClickMode::Hold => 1,
            ClickMode::Burst => 2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ClickMode::Toggle => "Toggle",
            ClickMode::Hold => "Hold",
            ClickMode::Burst => "Burst",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    Stopped,
    Running,
}

pub struct Scheduler {
    mode: ClickMode,
    state: RunState,
    burst_remaining: u32,
    burst_count: u32,
}

impl Scheduler {
    pub fn new(mode: ClickMode, burst_count: u32) -> Self {
        Self {
            mode,
            state: RunState::Stopped,
            burst_remaining: 0,
            burst_count: burst_count.max(1),
        }
    }

    pub fn mode(&self) -> ClickMode {
        self.mode
    }

    pub fn state(&self) -> RunState {
        self.state
    }

    pub fn set_mode(&mut self, mode: ClickMode) {
        self.mode = mode;
        self.state = RunState::Stopped;
        self.burst_remaining = 0;
    }

    pub fn set_burst_count(&mut self, count: u32) {
        self.burst_count = count.max(1);
    }

    pub fn toggle_ui(&mut self) {
        match self.mode {
            ClickMode::Toggle => self.toggle_running(),
            ClickMode::Hold => {}
            ClickMode::Burst => self.start_burst(),
        }
    }

    pub fn start(&mut self) {
        match self.mode {
            ClickMode::Toggle | ClickMode::Hold => {
                self.state = RunState::Running;
            }
            ClickMode::Burst => self.start_burst(),
        }
    }

    pub fn stop(&mut self) {
        self.state = RunState::Stopped;
        self.burst_remaining = 0;
    }

    pub fn on_hotkey(&mut self, action: HotkeyAction) {
        match self.mode {
            ClickMode::Toggle => {
                if action == HotkeyAction::Press {
                    self.toggle_running();
                }
            }
            ClickMode::Hold => match action {
                HotkeyAction::Press => self.state = RunState::Running,
                HotkeyAction::Release => {
                    self.state = RunState::Stopped;
                    self.burst_remaining = 0;
                }
            },
            ClickMode::Burst => {
                if action == HotkeyAction::Press {
                    self.start_burst();
                }
            }
        }
    }

    pub fn on_click_fired(&mut self) -> bool {
        if self.state != RunState::Running {
            return false;
        }

        match self.mode {
            ClickMode::Toggle | ClickMode::Hold => true,
            ClickMode::Burst => {
                if self.burst_remaining > 0 {
                    self.burst_remaining -= 1;
                    if self.burst_remaining == 0 {
                        self.state = RunState::Stopped;
                    }
                    true
                } else {
                    false
                }
            }
        }
    }

    fn toggle_running(&mut self) {
        self.state = match self.state {
            RunState::Stopped => RunState::Running,
            RunState::Running => RunState::Stopped,
        };
    }

    fn start_burst(&mut self) {
        self.state = RunState::Running;
        self.burst_remaining = self.burst_count;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_click_mode_from_index() {
        assert_eq!(ClickMode::from_index(0), ClickMode::Toggle);
        assert_eq!(ClickMode::from_index(1), ClickMode::Hold);
        assert_eq!(ClickMode::from_index(2), ClickMode::Burst);
        assert_eq!(ClickMode::from_index(999), ClickMode::Toggle); // Default
    }

    #[test]
    fn test_click_mode_to_index() {
        assert_eq!(ClickMode::Toggle.to_index(), 0);
        assert_eq!(ClickMode::Hold.to_index(), 1);
        assert_eq!(ClickMode::Burst.to_index(), 2);
    }

    #[test]
    fn test_click_mode_labels() {
        assert_eq!(ClickMode::Toggle.label(), "Toggle");
        assert_eq!(ClickMode::Hold.label(), "Hold");
        assert_eq!(ClickMode::Burst.label(), "Burst");
    }

    #[test]
    fn test_scheduler_initial_state() {
        let s = Scheduler::new(ClickMode::Toggle, 10);
        assert_eq!(s.mode(), ClickMode::Toggle);
        assert_eq!(s.state(), RunState::Stopped);
    }

    #[test]
    fn test_toggle_mode_flips_state() {
        let mut s = Scheduler::new(ClickMode::Toggle, 10);
        s.on_hotkey(HotkeyAction::Press);
        assert_eq!(s.state(), RunState::Running);
        s.on_hotkey(HotkeyAction::Press);
        assert_eq!(s.state(), RunState::Stopped);
    }

    #[test]
    fn test_toggle_mode_ignores_release() {
        let mut s = Scheduler::new(ClickMode::Toggle, 10);
        s.on_hotkey(HotkeyAction::Press);
        assert_eq!(s.state(), RunState::Running);
        s.on_hotkey(HotkeyAction::Release); // Should be ignored
        assert_eq!(s.state(), RunState::Running);
    }

    #[test]
    fn test_hold_mode_press_and_release() {
        let mut s = Scheduler::new(ClickMode::Hold, 10);

        s.on_hotkey(HotkeyAction::Press);
        assert_eq!(s.state(), RunState::Running);

        s.on_hotkey(HotkeyAction::Release);
        assert_eq!(s.state(), RunState::Stopped);
    }

    #[test]
    fn test_burst_fires_exact_count() {
        let mut s = Scheduler::new(ClickMode::Burst, 3);
        s.on_hotkey(HotkeyAction::Press);
        assert_eq!(s.state(), RunState::Running);

        assert!(s.on_click_fired()); // 1
        assert_eq!(s.state(), RunState::Running);

        assert!(s.on_click_fired()); // 2
        assert_eq!(s.state(), RunState::Running);

        assert!(s.on_click_fired()); // 3
        assert_eq!(s.state(), RunState::Stopped); // Should stop after burst
    }

    #[test]
    fn test_burst_count_minimum() {
        // Burst count should be at least 1
        let s = Scheduler::new(ClickMode::Burst, 0);
        let mut s_copy = s;
        s_copy.start_burst();
        assert!(s_copy.on_click_fired()); // Should fire at least once
    }

    #[test]
    fn test_set_mode_resets_state() {
        let mut s = Scheduler::new(ClickMode::Toggle, 10);
        s.start();
        assert_eq!(s.state(), RunState::Running);

        s.set_mode(ClickMode::Hold);
        assert_eq!(s.state(), RunState::Stopped); // Should reset
        assert_eq!(s.mode(), ClickMode::Hold);
    }

    #[test]
    fn test_set_burst_count() {
        let mut s = Scheduler::new(ClickMode::Burst, 5);
        s.set_burst_count(10);
        s.start();

        for _ in 0..10 {
            assert!(s.on_click_fired());
        }
        assert_eq!(s.state(), RunState::Stopped);
    }

    #[test]
    fn test_stop_clears_burst() {
        let mut s = Scheduler::new(ClickMode::Burst, 10);
        s.start();
        assert!(s.on_click_fired());

        s.stop();
        assert_eq!(s.state(), RunState::Stopped);

        // After stop, on_click_fired should return false
        assert!(!s.on_click_fired());
    }

    #[test]
    fn test_toggle_ui_in_toggle_mode() {
        let mut s = Scheduler::new(ClickMode::Toggle, 10);
        s.toggle_ui();
        assert_eq!(s.state(), RunState::Running);
        s.toggle_ui();
        assert_eq!(s.state(), RunState::Stopped);
    }

    #[test]
    fn test_toggle_ui_in_burst_mode() {
        let mut s = Scheduler::new(ClickMode::Burst, 3);
        s.toggle_ui();
        assert_eq!(s.state(), RunState::Running);
        assert!(s.on_click_fired());
    }

    #[test]
    fn test_toggle_ui_in_hold_mode() {
        let mut s = Scheduler::new(ClickMode::Hold, 10);
        s.toggle_ui(); // Should do nothing in Hold mode
        assert_eq!(s.state(), RunState::Stopped);
    }

    #[test]
    fn test_on_click_fired_when_stopped() {
        let mut s = Scheduler::new(ClickMode::Toggle, 10);
        assert!(!s.on_click_fired()); // Should return false when stopped
    }

    #[test]
    fn test_multiple_burst_activations() {
        let mut s = Scheduler::new(ClickMode::Burst, 2);

        // First burst
        s.start();
        assert!(s.on_click_fired());
        assert!(s.on_click_fired());
        assert_eq!(s.state(), RunState::Stopped);

        // Second burst
        s.start();
        assert!(s.on_click_fired());
        assert!(s.on_click_fired());
        assert_eq!(s.state(), RunState::Stopped);
    }
}
