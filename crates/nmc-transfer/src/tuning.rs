//! Automatic choice of how many files a job copies at the same time.
//!
//! A hill climber: every interval the throughput of the job is compared with
//! the previous one. While one more file at a time makes the transfer clearly
//! faster it keeps adding files; when it gets slower (or transient errors
//! appear) it takes one away and goes the other way. In between it holds the
//! level for a few intervals and then probes upward again, so a change in the
//! network or the disks is followed.

/// The fewest files at a time.
pub const MIN_PARALLELISM: usize = 1;
/// The most files at a time the tuner will try.
pub const MAX_PARALLELISM: usize = 16;

/// A change of throughput smaller than this counts as no change.
const SIGNIFICANT: f64 = 0.10;
/// Intervals to wait at a stable level before probing upward again.
const HOLD_INTERVALS: u32 = 3;

/// The state of the tuner of one job.
#[derive(Debug, Clone)]
pub struct Tuner {
    level: usize,
    last: Option<f64>,
    up: bool,
    /// Samples in a row without a significant change.
    stable: u32,
    /// Samples left to wait after errors before looking at the speed again.
    cooldown: u32,
}

impl Tuner {
    /// Starts at `level` files at a time.
    pub fn new(level: usize) -> Self {
        Tuner {
            level: level.clamp(MIN_PARALLELISM, MAX_PARALLELISM),
            last: None,
            up: true,
            stable: 0,
            cooldown: 0,
        }
    }

    /// The current level.
    pub fn level(&self) -> usize {
        self.level
    }

    /// Follows a level set by hand.
    pub fn reset(&mut self, level: usize) {
        *self = Tuner::new(level);
    }

    /// Takes the bytes per second measured over the last interval and the
    /// number of retried errors in it, and returns the level to use next.
    pub fn next(&mut self, bytes_per_second: f64, retries: u32) -> usize {
        if retries > 0 {
            // Errors: back off and do not climb again straight away.
            self.last = None;
            self.up = false;
            self.stable = 0;
            self.cooldown = HOLD_INTERVALS;
            self.level = self.level.saturating_sub(1).max(MIN_PARALLELISM);
            return self.level;
        }
        if bytes_per_second <= 0.0 {
            return self.level; // nothing moved (planning, a pause): no information
        }
        if self.cooldown > 0 {
            self.cooldown -= 1;
            self.last = Some(bytes_per_second);
            return self.level;
        }
        let Some(last) = self.last else {
            self.last = Some(bytes_per_second);
            self.step();
            return self.level;
        };
        let change = (bytes_per_second - last) / last;
        self.last = Some(bytes_per_second);
        if change > SIGNIFICANT {
            self.stable = 0;
            self.step(); // it helped: go on the same way
        } else if change < -SIGNIFICANT {
            self.stable = 0;
            self.up = !self.up; // it hurt: go the other way
            self.step();
        } else {
            self.stable += 1;
            if self.stable > HOLD_INTERVALS {
                self.stable = 0;
                self.up = true;
                self.step(); // probe upward again
            }
        }
        self.level
    }

    fn step(&mut self) {
        let next = if self.up { self.level + 1 } else { self.level.saturating_sub(1) };
        self.level = next.clamp(MIN_PARALLELISM, MAX_PARALLELISM);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn climbs_while_it_helps_and_stops_when_it_does_not() {
        let mut tuner = Tuner::new(2);
        assert_eq!(tuner.next(10.0, 0), 3); // first sample: probe up
        assert_eq!(tuner.next(15.0, 0), 4); // +50 %: go on
        assert_eq!(tuner.next(20.0, 0), 5);
        assert_eq!(tuner.next(20.5, 0), 5); // no gain: hold
        assert_eq!(tuner.next(20.4, 0), 5);
    }

    #[test]
    fn turns_around_when_it_gets_slower() {
        let mut tuner = Tuner::new(4);
        tuner.next(100.0, 0); // 5
        assert_eq!(tuner.next(50.0, 0), 4); // worse: back down
        assert_eq!(tuner.next(80.0, 0), 3); // better while going down: go on
    }

    #[test]
    fn errors_lower_the_level() {
        let mut tuner = Tuner::new(6);
        assert_eq!(tuner.next(100.0, 2), 5);
        assert_eq!(tuner.next(100.0, 0), 5); // first sample after errors: no climb back at once
    }

    #[test]
    fn stays_within_bounds() {
        let mut tuner = Tuner::new(MAX_PARALLELISM);
        assert_eq!(tuner.next(10.0, 0), MAX_PARALLELISM);
        let mut tuner = Tuner::new(1);
        tuner.next(100.0, 1);
        assert_eq!(tuner.level(), 1);
        assert_eq!(tuner.next(0.0, 0), 1); // idle: unchanged
    }

    #[test]
    fn probes_upward_again_after_holding() {
        let mut tuner = Tuner::new(3);
        tuner.next(10.0, 0); // 4
        for _ in 0..=HOLD_INTERVALS {
            tuner.next(10.2, 0);
        }
        assert_eq!(tuner.level(), 5);
    }
}
