use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TryRecvError};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("failed to spawn click engine thread: {0}")]
    ThreadSpawn(#[from] std::io::Error),
}

use crate::backend::{ClickDevice, MouseButton, VirtualMouse};
use crate::core::scheduler::{ClickMode, RunState, Scheduler};
use crate::core::timing::{TimingConfig, TimingEngine};
use crate::system::hotkey::HotkeyEvent;

const MIN_STATS_INTERVAL: Duration = Duration::from_millis(16);
const SPIN_TAIL_THRESHOLD: Duration = Duration::from_millis(2);

#[derive(Debug, Clone)]
pub enum EngineCommand {
    Start,
    Stop,
    Toggle,
    SetCps(u32),
    SetMode(ClickMode),
    SetButton(MouseButton),
    SetBurstCount(u32),
    SetJitter(bool),
    AttachHotkey(Receiver<HotkeyEvent>),
    RecreateDevice,
    Shutdown,
}

#[derive(Debug, Clone)]
pub struct EngineStats {
    pub running: bool,
    pub total_clicks: u64,
    pub live_cps: f64,
    pub target_cps: u32,
    pub device_active: bool,
    pub device_error: Option<String>,
    pub mode: ClickMode,
}

pub struct ClickEngine {
    cmd_tx: Sender<EngineCommand>,
    stats_rx: Receiver<EngineStats>,
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl ClickEngine {
    pub fn start(initial: EngineSettings) -> Result<Self, EngineError> {
        Self::start_with_factory(initial, || {
            VirtualMouse::new()
                .map(|m| Box::new(m) as Box<dyn ClickDevice>)
                .map_err(|e| e.to_string())
        })
    }

    pub fn start_with_device<D: ClickDevice + 'static>(
        initial: EngineSettings,
        device: D,
    ) -> Result<Self, EngineError> {
        Self::start_with_factory(
            initial,
            move || Ok(Box::new(device) as Box<dyn ClickDevice>),
        )
    }

    fn start_with_factory<F>(initial: EngineSettings, factory: F) -> Result<Self, EngineError>
    where
        F: FnOnce() -> Result<Box<dyn ClickDevice>, String> + Send + 'static,
    {
        let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
        let (stats_tx, stats_rx) = crossbeam_channel::unbounded();
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_flag = Arc::clone(&shutdown);

        let handle = thread::Builder::new()
            .name("click-engine".into())
            .spawn(move || engine_loop(cmd_rx, stats_tx, shutdown_flag, initial, factory))?;

        Ok(Self {
            cmd_tx,
            stats_rx,
            shutdown,
            handle: Some(handle),
        })
    }

    pub fn send(&self, cmd: EngineCommand) {
        let _ = self.cmd_tx.send(cmd);
    }

    pub fn try_recv_stats(&self) -> Option<EngineStats> {
        let mut latest = None;
        while let Ok(stats) = self.stats_rx.try_recv() {
            latest = Some(stats);
        }
        latest
    }

    pub fn shutdown(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let _ = self.cmd_tx.send(EngineCommand::Shutdown);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for ClickEngine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[derive(Debug, Clone)]
pub struct EngineSettings {
    pub cps: u32,
    pub mode: ClickMode,
    pub button: MouseButton,
    pub burst_count: u32,
    pub jitter_enabled: bool,
}

fn engine_loop<F>(
    cmd_rx: Receiver<EngineCommand>,
    stats_tx: Sender<EngineStats>,
    shutdown: Arc<AtomicBool>,
    mut settings: EngineSettings,
    device_factory: F,
) where
    F: FnOnce() -> Result<Box<dyn ClickDevice>, String>,
{
    let mut mouse = match device_factory() {
        Ok(m) => m,
        Err(e) => {
            let _ = stats_tx.send(EngineStats {
                running: false,
                total_clicks: 0,
                live_cps: 0.0,
                target_cps: settings.cps,
                device_active: false,
                device_error: Some(e),
                mode: settings.mode,
            });
            return;
        }
    };

    let mut scheduler = Scheduler::new(settings.mode, settings.burst_count);
    let mut timing = TimingEngine::new(timing_config(&settings));
    let mut total_clicks: u64 = 0;
    let mut click_window: VecDeque<Instant> = VecDeque::with_capacity(256);
    let mut deadline: Option<Instant> = None;
    let mut hotkey_rx: Option<Receiver<HotkeyEvent>> = None;
    let mut last_stats_emit = Instant::now() - MIN_STATS_INTERVAL;

    emit_stats(
        &stats_tx,
        &scheduler,
        mouse.as_ref(),
        &settings,
        total_clicks,
        &mut click_window,
        &mut last_stats_emit,
    );

    while !shutdown.load(Ordering::SeqCst) {
        // Service hotkey events on every iteration for responsiveness
        if let Some(rx) = hotkey_rx.as_ref() {
            while let Ok(event) = rx.try_recv() {
                scheduler.on_hotkey(event.action);
                if scheduler.state() == RunState::Running && deadline.is_none() {
                    timing.reset();
                    deadline = Some(timing.schedule_next());
                } else if scheduler.state() == RunState::Stopped {
                    deadline = None;
                    timing.reset();
                }
                emit_stats(
                    &stats_tx,
                    &scheduler,
                    mouse.as_ref(),
                    &settings,
                    total_clicks,
                    &mut click_window,
                    &mut last_stats_emit,
                );
            }
        }

        if scheduler.state() == RunState::Running {
            if let Some(dl) = deadline {
                // Wait precisely until the deadline
                wait_until_deadline(dl, &shutdown);

                // Check if shutdown was requested during wait
                if shutdown.load(Ordering::SeqCst) {
                    break;
                }

                // Also drain any commands that arrived during the wait
                drain_commands(
                    &cmd_rx,
                    &mut settings,
                    &mut scheduler,
                    &mut timing,
                    mouse.as_mut(),
                    &mut deadline,
                    &mut hotkey_rx,
                );
                if scheduler.state() != RunState::Running {
                    emit_stats(
                        &stats_tx,
                        &scheduler,
                        mouse.as_ref(),
                        &settings,
                        total_clicks,
                        &mut click_window,
                        &mut last_stats_emit,
                    );
                    continue;
                }

                // Deadline reached - fire the click
                match mouse.click(settings.button) {
                    Ok(()) => {
                        if scheduler.on_click_fired() {
                            total_clicks += 1;
                            click_window.push_back(Instant::now());
                        }
                    }
                    Err(_) => {
                        tracing::warn!("Click failed, attempting device recreation");
                        if let Err(e) = mouse.recreate() {
                            tracing::error!("Failed to recreate device: {}", e);
                        } else {
                            tracing::info!("Device recreated successfully");
                        }
                    }
                }

                // Schedule next deadline from the IDEAL deadline, not from now
                if scheduler.state() == RunState::Running {
                    deadline = Some(timing.schedule_next_from(dl));
                } else {
                    deadline = None;
                    timing.reset();
                }

                emit_stats(
                    &stats_tx,
                    &scheduler,
                    mouse.as_ref(),
                    &settings,
                    total_clicks,
                    &mut click_window,
                    &mut last_stats_emit,
                );
            } else {
                // Running but no deadline set - shouldn't happen, but recover
                timing.reset();
                deadline = Some(timing.schedule_next());
            }
        } else {
            // Not running — block until a command arrives or we need to emit stats
            match safe_recv_timeout(&cmd_rx, Duration::from_millis(100)) {
                Ok(EngineCommand::Shutdown) => break,
                Ok(cmd) => {
                    apply_command(
                        cmd,
                        &mut settings,
                        &mut scheduler,
                        &mut timing,
                        mouse.as_mut(),
                        &mut deadline,
                        &mut hotkey_rx,
                    );
                    emit_stats(
                        &stats_tx,
                        &scheduler,
                        mouse.as_ref(),
                        &settings,
                        total_clicks,
                        &mut click_window,
                        &mut last_stats_emit,
                    );
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    // Periodic stats emission keeps UI alive while idle
                    emit_stats(
                        &stats_tx,
                        &scheduler,
                        mouse.as_ref(),
                        &settings,
                        total_clicks,
                        &mut click_window,
                        &mut last_stats_emit,
                    );
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            }
        }
    }
}

/// A safe replacement for crossbeam-channel 0.5's broken `recv_timeout`.
/// crossbeam-channel 0.5 internally uses `thread::park_timeout()` for blocking,
/// but a stale park-permit bug causes it to spin at 100% CPU instead of sleeping.
/// This function uses `thread::sleep()` which properly blocks on Linux.
fn safe_recv_timeout<T>(rx: &Receiver<T>, timeout: Duration) -> Result<T, RecvTimeoutError> {
    let deadline = Instant::now() + timeout;
    loop {
        match rx.try_recv() {
            Ok(msg) => return Ok(msg),
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => return Err(RecvTimeoutError::Disconnected),
        }

        let now = Instant::now();
        if now >= deadline {
            return Err(RecvTimeoutError::Timeout);
        }

        // Sleep in chunks to keep responsiveness reasonable.
        let remaining = deadline - now;
        let sleep_dur = std::cmp::min(remaining, Duration::from_millis(10));
        std::thread::sleep(sleep_dur);
    }
}

/// Wait until the given deadline using OS-level sleep for coarse waits
/// and a short spin-loop for the final sub-2ms tail to preserve high-CPS accuracy.
fn wait_until_deadline(dl: Instant, shutdown: &AtomicBool) {
    loop {
        if shutdown.load(Ordering::SeqCst) {
            return;
        }
        let remaining = dl.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return;
        }
        if remaining > SPIN_TAIL_THRESHOLD {
            let sleep_for =
                std::cmp::min(remaining - SPIN_TAIL_THRESHOLD, Duration::from_millis(50));
            std::thread::sleep(sleep_for);
        } else {
            while !shutdown.load(Ordering::SeqCst) && Instant::now() < dl {
                std::hint::spin_loop();
            }
            return;
        }
    }
}

fn drain_commands(
    cmd_rx: &Receiver<EngineCommand>,
    settings: &mut EngineSettings,
    scheduler: &mut Scheduler,
    timing: &mut TimingEngine,
    mouse: &mut dyn ClickDevice,
    deadline: &mut Option<Instant>,
    hotkey_rx: &mut Option<Receiver<HotkeyEvent>>,
) {
    while let Ok(cmd) = cmd_rx.try_recv() {
        match cmd {
            EngineCommand::Shutdown => {} // handled by the main loop
            cmd => {
                apply_command(cmd, settings, scheduler, timing, mouse, deadline, hotkey_rx);
            }
        }
    }
}

fn apply_command(
    cmd: EngineCommand,
    settings: &mut EngineSettings,
    scheduler: &mut Scheduler,
    timing: &mut TimingEngine,
    mouse: &mut dyn ClickDevice,
    deadline: &mut Option<Instant>,
    hotkey_rx: &mut Option<Receiver<HotkeyEvent>>,
) {
    match cmd {
        EngineCommand::Start => {
            scheduler.start();
            if scheduler.state() == RunState::Running {
                timing.reset();
                *deadline = Some(timing.schedule_next());
            }
        }
        EngineCommand::Stop => {
            scheduler.stop();
            *deadline = None;
            timing.reset();
        }
        EngineCommand::Toggle => {
            scheduler.toggle_ui();
            if scheduler.state() == RunState::Running {
                timing.reset();
                *deadline = Some(timing.schedule_next());
            } else {
                *deadline = None;
                timing.reset();
            }
        }
        EngineCommand::SetCps(cps) => {
            settings.cps = cps.clamp(1, 1000);
            timing.set_config(timing_config(settings));
        }
        EngineCommand::SetMode(mode) => {
            settings.mode = mode;
            scheduler.set_mode(mode);
            *deadline = None;
            timing.reset();
        }
        EngineCommand::SetButton(button) => settings.button = button,
        EngineCommand::SetBurstCount(count) => {
            settings.burst_count = count;
            scheduler.set_burst_count(count);
        }
        EngineCommand::SetJitter(enabled) => {
            settings.jitter_enabled = enabled;
            timing.set_config(timing_config(settings));
        }
        EngineCommand::AttachHotkey(rx) => {
            while rx.try_recv().is_ok() {}
            *hotkey_rx = Some(rx);
        }
        EngineCommand::RecreateDevice => {
            let _ = mouse.recreate();
        }
        EngineCommand::Shutdown => {}
    }
}

fn timing_config(settings: &EngineSettings) -> TimingConfig {
    TimingConfig {
        cps: settings.cps,
        jitter_enabled: settings.jitter_enabled,
    }
}

fn compute_live_cps(window: &mut VecDeque<Instant>) -> f64 {
    let cutoff = Instant::now() - Duration::from_secs(1);
    while let Some(&front) = window.front() {
        if front < cutoff {
            window.pop_front();
        } else {
            break;
        }
    }
    window.len() as f64
}

fn emit_stats(
    stats_tx: &Sender<EngineStats>,
    scheduler: &Scheduler,
    mouse: &dyn ClickDevice,
    settings: &EngineSettings,
    total_clicks: u64,
    click_window: &mut VecDeque<Instant>,
    last_stats_emit: &mut Instant,
) {
    let now = Instant::now();
    if now - *last_stats_emit < MIN_STATS_INTERVAL {
        return;
    }
    *last_stats_emit = now;

    let _ = stats_tx.send(EngineStats {
        running: scheduler.state() == RunState::Running,
        total_clicks,
        live_cps: compute_live_cps(click_window),
        target_cps: settings.cps,
        device_active: mouse.is_active(),
        device_error: mouse.last_error().map(str::to_string),
        mode: settings.mode,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::uinput_driver::UInputError;
    use std::sync::atomic::AtomicU64;

    struct FakeDevice {
        clicks: Arc<AtomicU64>,
        active: bool,
    }

    impl ClickDevice for FakeDevice {
        fn is_active(&self) -> bool {
            self.active
        }
        fn last_error(&self) -> Option<&str> {
            None
        }
        fn recreate(&mut self) -> Result<(), UInputError> {
            self.active = true;
            Ok(())
        }
        fn click(&mut self, _button: MouseButton) -> Result<(), UInputError> {
            self.clicks.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[test]
    fn test_click_engine_burst_with_jitter() {
        let clicks = Arc::new(AtomicU64::new(0));
        let device = FakeDevice {
            clicks: Arc::clone(&clicks),
            active: true,
        };
        let settings = EngineSettings {
            cps: 1000,
            mode: ClickMode::Burst,
            button: MouseButton::Left,
            burst_count: 5,
            jitter_enabled: true,
        };

        let mut engine = ClickEngine::start_with_device(settings, device).unwrap();
        engine.send(EngineCommand::Start);
        thread::sleep(Duration::from_millis(80));
        engine.shutdown();

        assert_eq!(clicks.load(Ordering::SeqCst), 5);
    }

    #[test]
    fn test_compute_live_cps_exceeds_128() {
        let mut window = VecDeque::new();
        let now = Instant::now();
        for _ in 0..250 {
            window.push_back(now);
        }
        assert_eq!(compute_live_cps(&mut window), 250.0);
    }
}
