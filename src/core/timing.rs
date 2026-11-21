use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct TimingConfig {
    pub cps: u32,
    pub jitter_enabled: bool,
}

pub struct TimingEngine {
    config: TimingConfig,
}

impl TimingEngine {
    pub fn new(config: TimingConfig) -> Self {
        Self { config }
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
            apply_jitter(base_interval)
        } else {
            base_interval
        }
    }

    pub fn reset(&mut self) {}
}

/// Calculate interval between clicks based on CPS
pub fn interval_for_cps(cps: u32) -> Duration {
    if cps == 0 {
        return Duration::from_secs(1);
    }
    Duration::from_micros(1_000_000 / cps as u64)
}

/// Add ±15% random jitter to a base duration.
pub fn apply_jitter(base: Duration) -> Duration {
    use rand::Rng;
    let base_us = base.as_micros() as i64;
    let delta = (base_us * 15) / 100;
    if delta <= 0 {
        return base;
    }
    let mut rng = rand::thread_rng();
    let offset = rng.gen_range(-delta..=delta);
    let jittered_us = (base_us + offset).max(1) as u64;
    Duration::from_micros(jittered_us)
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
        let base = Duration::from_millis(100);

        for _ in 0..100 {
            let result = apply_jitter(base);
            let result_ms = result.as_millis();
            assert!((85..=115).contains(&result_ms), "result_ms={result_ms}");
        }
    }

    #[test]
    fn test_apply_jitter_sub_millisecond_safe() {
        let base = Duration::from_micros(5);
        let result = apply_jitter(base);
        assert_eq!(result, base);
    }

    #[test]
    fn test_schedule_next_from() {
        let config = TimingConfig {
            cps: 100,
            jitter_enabled: false,
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
        assert!(
            diff < Duration::from_millis(1),
            "next={:?}, expected={:?}",
            next,
            expected
        );
    }
}
