use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct TimingConfig {
    pub cps: u32,
    pub jitter_enabled: bool,
    pub jitter_min_ms: u32,
    pub jitter_max_ms: u32,
    pub smoothing_enabled: bool,
}

pub struct TimingEngine {
    config: TimingConfig,
    sleeper: PreciseSleeper,
}

impl TimingEngine {
    pub fn new(config: TimingConfig) -> Self {
        Self {
            config,
            sleeper: PreciseSleeper::new(),
        }
    }

    pub fn set_config(&mut self, config: TimingConfig) {
        self.config = config;
    }


    /// Schedule the next click deadline relative to now
    pub fn schedule_next(&mut self) -> Instant {
        let interval = self.compute_interval();
        Instant::now() + interval
    }

    /// Schedule the next click deadline relative to the previous deadline
    /// This ensures consistent CPS regardless of click execution duration
    pub fn schedule_next_from(&mut self, last_deadline: Instant) -> Instant {
        let interval = self.compute_interval();
        // Use the last deadline + interval, but ensure we don't schedule
        // in the past (capped to now)
        let next = last_deadline + interval;
        let now = Instant::now();
        if next > now {
            next
        } else {
            // We're falling behind; just schedule from now to catch up
            now + interval
        }
    }

    fn compute_interval(&self) -> Duration {
        let base_interval = interval_for_cps(self.config.cps);
        if self.config.jitter_enabled {
            apply_jitter(
                base_interval,
                self.config.jitter_min_ms,
                self.config.jitter_max_ms,
            )
        } else {
            base_interval
        }
    }

    pub fn wait_for_next_click(&mut self) {
        let interval = self.compute_interval();
        self.sleeper.sleep(interval);
    }

    pub fn reset(&mut self) {
        self.sleeper.reset();
    }
}

/// Calculate interval between clicks based on CPS
pub fn interval_for_cps(cps: u32) -> Duration {
    if cps == 0 {
        return Duration::from_secs(1);
    }
    Duration::from_micros(1_000_000 / cps as u64)
}

/// Add random jitter to a base duration
pub fn apply_jitter(base: Duration, min_ms: u32, max_ms: u32) -> Duration {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let jitter_ms = rng.gen_range(min_ms..=max_ms);
    base.saturating_add(Duration::from_millis(jitter_ms as u64))
}

/// Precise sleep that compensates for drift
pub struct PreciseSleeper {
    last_tick: Instant,
}

impl PreciseSleeper {
    pub fn new() -> Self {
        Self {
            last_tick: Instant::now(),
        }
    }

    pub fn sleep(&mut self, target_interval: Duration) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_tick);

        if elapsed < target_interval {
            let sleep_duration = target_interval - elapsed;
            std::thread::sleep(sleep_duration);
        }

        self.last_tick = Instant::now();
    }

    pub fn reset(&mut self) {
        self.last_tick = Instant::now();
    }
}

impl Default for PreciseSleeper {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interval_for_cps() {
        // Test common CPS values
        assert_eq!(interval_for_cps(1), Duration::from_secs(1));
        assert_eq!(interval_for_cps(10), Duration::from_millis(100));
        assert_eq!(interval_for_cps(20), Duration::from_millis(50));
        assert_eq!(interval_for_cps(100), Duration::from_millis(10));
        assert_eq!(interval_for_cps(1000), Duration::from_micros(1000));
    }

    #[test]
    fn test_interval_for_zero_cps() {
        // Edge case: 0 CPS should return 1 second (safe fallback)
        assert_eq!(interval_for_cps(0), Duration::from_secs(1));
    }

    #[test]
    fn test_apply_jitter_range() {
        let base = Duration::from_millis(50);
        let min_ms = 5;
        let max_ms = 20;

        // Run multiple times to test randomness
        for _ in 0..100 {
            let result = apply_jitter(base, min_ms, max_ms);
            let result_ms = result.as_millis();

            // Should be between base + min and base + max
            assert!(result_ms >= 55, "result_ms={} should be >= 55", result_ms);
            assert!(result_ms <= 70, "result_ms={} should be <= 70", result_ms);
        }
    }

    #[test]
    fn test_apply_jitter_zero_range() {
        let base = Duration::from_millis(50);
        let result = apply_jitter(base, 0, 0);

        // With 0 jitter, should return base duration
        assert_eq!(result, base);
    }

    #[test]
    fn test_precise_sleeper_reset() {
        let mut sleeper = PreciseSleeper::new();
        let initial_tick = sleeper.last_tick;

        std::thread::sleep(Duration::from_millis(10));
        sleeper.reset();

        assert!(sleeper.last_tick > initial_tick);
    }

    #[test]
    fn test_precise_sleeper_default() {
        let sleeper = PreciseSleeper::default();
        let now = Instant::now();

        // Should be initialized to approximately now
        let diff = now.duration_since(sleeper.last_tick);
        assert!(diff < Duration::from_millis(10));
    }

    #[test]
    fn test_schedule_next_from() {
        let config = TimingConfig {
            cps: 100,
            jitter_enabled: false,
            jitter_min_ms: 0,
            jitter_max_ms: 0,
            smoothing_enabled: false,
        };
        let mut timing = TimingEngine::new(config);

        // Schedule from a specific deadline
        let base = Instant::now();
        let next = timing.schedule_next_from(base);
        let expected = base + Duration::from_millis(10);

        // Should be close to base + interval (within 1ms for test timing)
        let diff = if next > expected {
            next - expected
        } else {
            expected - next
        };
        assert!(diff < Duration::from_millis(1), "next={:?}, expected={:?}", next, expected);
    }
}
