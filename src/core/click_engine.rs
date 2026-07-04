use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};

use crate::backend::{MouseButton, VirtualMouse};
use crate::core::scheduler::{ClickMode, RunState, Scheduler};
use crate::core::timing::{TimingConfig, TimingEngine};
use crate::system::hotkey::HotkeyEvent;

/// Minimum interval where we can safely use OS sleep + spin hybrid.
/// Below this, we pure-spin to avoid Linux scheduler tick imprecision (~4ms).
const HYBRID_THRESHOLD_MS: u64 = 20; // 50 CPS
/// How long to spin after sleeping before the deadline
const SPIN_WINDOW_MS: u64 = 5;

#[derive(Debug, Clone)]
pub enum EngineCommand {
    Start,
    Stop,
    Toggle,
    SetCps(u32),
    SetMode(ClickMode),
    SetButton(MouseButton),
    SetBurstCount(u32),
    SetJitter {
        enabled: bool,
        min_ms: u32,
        max_ms: u32,
    },
    SetSmoothing(bool),
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
    pub fn start(initial: EngineSettings) -> Result<Self, String> {
        let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
        let (stats_tx, stats_rx) = crossbeam_channel::unbounded();
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_flag = Arc::clone(&shutdown);

        let handle = thread::Builder::new()
            .name("click-engine".into())
            .spawn(move || engine_loop(cmd_rx, stats_tx, shutdown_flag, initial))
            .map_err(|e| e.to_string())?;

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
    pub jitter_min_ms: u32,
    pub jitter_max_ms: u32,
    pub smoothing_enabled: bool,
}

fn engine_loop(
    cmd_rx: Receiver<EngineCommand>,
    stats_tx: Sender<EngineStats>,
    shutdown: Arc<AtomicBool>,
    mut settings: EngineSettings,
) {
    let mut mouse = match VirtualMouse::new() {
        Ok(m) => m,
        Err(e) => {
            let _ = stats_tx.send(EngineStats {
                running: false,
                total_clicks: 0,
                live_cps: 0.0,
                target_cps: settings.cps,
                device_active: false,
                device_error: Some(e.to_string()),
                mode: settings.mode,
            });
            return;
        }
    };

    let mut scheduler = Scheduler::new(settings.mode, settings.burst_count);
    let mut timing = TimingEngine::new(timing_config(&settings));
    let mut total_clicks: u64 = 0;
    let mut click_window: [Option<Instant>; 128] = [None; 128];
    let mut click_window_idx: usize = 0;
    let mut deadline: Option<Instant> = None;
    let mut hotkey_rx: Option<Receiver<HotkeyEvent>> = None;

    emit_stats(
        &stats_tx,
        &scheduler,
        &mouse,
        &settings,
        total_clicks,
        &click_window,
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
                    &mouse,
                    &settings,
                    total_clicks,
                    &click_window,
                );
            }
        }

        if scheduler.state() == RunState::Running {
            if let Some(dl) = deadline {
                // Wait precisely until the deadline
                wait_until_deadline(dl, &cmd_rx, &shutdown);

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
                    &mut mouse,
                    &mut deadline,
                    &mut hotkey_rx,
                );
                if scheduler.state() != RunState::Running {
                    emit_stats(
                        &stats_tx,
                        &scheduler,
                        &mouse,
                        &settings,
                        total_clicks,
                        &click_window,
                    );
                    continue;
                }

                // Deadline reached - fire the click
                match mouse.click(settings.button) {
                    Ok(()) => {
                        if scheduler.on_click_fired() {
                            total_clicks += 1;
                            click_window[click_window_idx] = Some(Instant::now());
                            click_window_idx = (click_window_idx + 1) % click_window.len();
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
                    &mouse,
                    &settings,
                    total_clicks,
                    &click_window,
                );
            } else {
                // Running but no deadline set - shouldn't happen, but recover
                timing.reset();
                deadline = Some(timing.schedule_next());
            }
        } else {
            // Not running - just wait for commands
            // Use a longer poll interval when idle
            let poll_deadline = Instant::now() + Duration::from_millis(10);
            while Instant::now() < poll_deadline {
                match cmd_rx.try_recv() {
                    Ok(EngineCommand::Shutdown) => break,
                    Ok(cmd) => {
                        apply_command(
                            cmd,
                            &mut settings,
                            &mut scheduler,
                            &mut timing,
                            &mut mouse,
                            &mut deadline,
                            &mut hotkey_rx,
                        );
                        emit_stats(
                            &stats_tx,
                            &scheduler,
                            &mouse,
                            &settings,
                            total_clicks,
                            &click_window,
                        );
                    }
                    Err(crossbeam_channel::TryRecvError::Empty) => {
                        std::hint::spin_loop();
                    }
                    Err(crossbeam_channel::TryRecvError::Disconnected) => break,
                }
            }
        }
    }
}

/// Wait precisely until the given deadline.
/// Uses a hybrid approach: OS sleep for most of the wait when the remaining
/// time is large enough, then busy-spin for the last few ms for precision.
/// For short waits (< HYBRID_THRESHOLD_MS), pure-spin to avoid Linux's
/// coarse scheduler tick (~4ms on most distros) from blowing the budget.
fn wait_until_deadline(
    dl: Instant,
    cmd_rx: &Receiver<EngineCommand>,
    shutdown: &AtomicBool,
) {
    loop {
        let now = Instant::now();
        if now >= dl {
            return;
        }
        let remaining = dl - now;
        let remaining_ms = remaining.as_millis() as u64;

        if remaining_ms >= HYBRID_THRESHOLD_MS {
            // We have enough time to sleep + spin safely
            let spin = Duration::from_millis(SPIN_WINDOW_MS);
            let sleep_for = remaining - spin;

            // Use recv_timeout as our sleep mechanism so we can also
            // catch command/shutdown signals while sleeping
            match cmd_rx.recv_timeout(sleep_for) {
                Ok(EngineCommand::Shutdown) => {
                    shutdown.store(true, Ordering::SeqCst);
                    return;
                }
                Ok(_cmd) => {
                    // Don't process commands here to avoid recursive processing;
                    // they'll be picked up by the drain after wait returns.
                    // Just continue the wait.
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    // Sleep completed normally
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    shutdown.store(true, Ordering::SeqCst);
                    return;
                }
            }
            // Fall through to spin after sleep
        }
        // Pure busy-spin: this gives microsecond precision regardless of
        // the OS scheduler tick granularity
        while Instant::now() < dl {
            // Periodically check for commands/shutdown during spin
            // to remain responsive
            match cmd_rx.try_recv() {
                Ok(EngineCommand::Shutdown) => {
                    shutdown.store(true, Ordering::SeqCst);
                    return;
                }
                Ok(_cmd) => {
                    // Command received but deferred; continue spinning
                }
                _ => {}
            }
            std::hint::spin_loop();
        }
        return;
    }
}

fn drain_commands(
    cmd_rx: &Receiver<EngineCommand>,
    settings: &mut EngineSettings,
    scheduler: &mut Scheduler,
    timing: &mut TimingEngine,
    mouse: &mut VirtualMouse,
    deadline: &mut Option<Instant>,
    hotkey_rx: &mut Option<Receiver<HotkeyEvent>>,
) {
    while let Ok(cmd) = cmd_rx.try_recv() {
        match cmd {
            EngineCommand::Shutdown => {} // handled by the main loop
            cmd => {
                apply_command(
                    cmd,
                    settings,
                    scheduler,
                    timing,
                    mouse,
                    deadline,
                    hotkey_rx,
                );
            }
        }
    }
}

fn apply_command(
    cmd: EngineCommand,
    settings: &mut EngineSettings,
    scheduler: &mut Scheduler,
    timing: &mut TimingEngine,
    mouse: &mut VirtualMouse,
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
        EngineCommand::SetJitter {
            enabled,
            min_ms,
            max_ms,
        } => {
            settings.jitter_enabled = enabled;
            settings.jitter_min_ms = min_ms;
            settings.jitter_max_ms = max_ms;
            timing.set_config(timing_config(settings));
        }
        EngineCommand::SetSmoothing(enabled) => {
            settings.smoothing_enabled = enabled;
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
        jitter_min_ms: settings.jitter_min_ms,
        jitter_max_ms: settings.jitter_max_ms,
        smoothing_enabled: settings.smoothing_enabled,
    }
}

fn compute_live_cps(window: &[Option<Instant>]) -> f64 {
    let cutoff = Instant::now() - Duration::from_secs(1);
    window
        .iter()
        .filter_map(|t| *t)
        .filter(|t| *t >= cutoff)
        .count() as f64
}

fn emit_stats(
    stats_tx: &Sender<EngineStats>,
    scheduler: &Scheduler,
    mouse: &VirtualMouse,
    settings: &EngineSettings,
    total_clicks: u64,
    click_window: &[Option<Instant>],
) {
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