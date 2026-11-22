//! Automatic choice of how many files a job copies at the same time.
//!
//! The controller looks at the measured throughput every interval (a moving
//! average over a few seconds) and works like the congestion control of a
//! network connection:
//!
//! - **Slow start.** It starts low and doubles the number of files while each
//!   doubling makes the transfer clearly faster.
//! - **Additive increase, careful probing.** Once more files stop helping it
//!   holds the level, and now and then tries one more; if the speed does not
//!   rise it goes back, so extra files never stay when they do not pay.
//! - **Multiplicative decrease.** Errors (retries) cut the level by a factor
//!   at once. Errors at a single file make it wait for the server and try
//!   again later, with a longer wait each time.
//! - **Per-server limits.** The level never goes above the limit of the
//!   server (set on the connection or learned from repeated errors).
//!
//! The *profile* sets how far and how boldly it goes.

use std::fmt;

/// The fewest files at a time (more than zero while a job is running; zero
/// only while the controller waits for a server).
pub const MIN_PARALLELISM: usize = 1;
/// The most files at a time, by hand or automatic.
pub const MAX_PARALLELISM: usize = 16;

/// Ticks to wait for a server after the first failure at one file.
const FIRST_WAIT: u32 = 2;
/// The longest wait for a server, in ticks.
const LONGEST_WAIT: u32 = 32;
/// Errors at two or more files that make the controller suggest a server limit.
const LIMIT_AFTER_ERRORS: u32 = 2;

/// How boldly the automatic mode raises the number of files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Profile {
    /// A good speed without overloading anything.
    #[default]
    Balanced,
    /// As fast as possible: more files, tries more often.
    MaxSpeed,
    /// Few files and gentle: leaves bandwidth and the server free.
    Discreet,
}

impl Profile {
    /// The most files the automatic mode tries.
    pub fn max_level(self) -> usize {
        match self {
            Profile::Balanced => 8,
            Profile::MaxSpeed => MAX_PARALLELISM,
            Profile::Discreet => 3,
        }
    }

    /// The first level of a job in automatic mode.
    pub fn start_level(self) -> usize {
        match self {
            Profile::Balanced | Profile::MaxSpeed => 2,
            Profile::Discreet => 1,
        }
    }

    /// The gain in throughput that counts as "more files help".
    fn threshold(self) -> f64 {
        match self {
            Profile::Balanced => 0.08,
            Profile::MaxSpeed => 0.04,
            Profile::Discreet => 0.15,
        }
    }

    /// Ticks at a steady level before trying one file more.
    fn hold(self) -> u32 {
        match self {
            Profile::Balanced => 4,
            Profile::MaxSpeed => 2,
            Profile::Discreet => 8,
        }
    }

    /// What is left of the level after errors.
    fn decrease(self) -> f64 {
        match self {
            Profile::Balanced => 0.7,
            Profile::MaxSpeed => 0.75,
            Profile::Discreet => 0.5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    SlowStart,
    Steady,
}

/// A level being tried: where to go back to, and the speed to beat.
#[derive(Debug, Clone, Copy)]
struct Probe {
    previous: usize,
    base: f64,
}

/// What the controller decided in one interval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    /// Files at a time from now on (0: waiting for the server).
    pub level: usize,
    /// Why the level changed, in words (`None` when it did not).
    pub reason: Option<String>,
    /// A limit of simultaneous files for the server, learned from errors.
    pub suggest_limit: Option<usize>,
}

/// The state of the controller of one job.
#[derive(Debug, Clone)]
pub struct Tuner {
    profile: Profile,
    level: usize,
    /// Moving average of the throughput in bytes per second.
    average: Option<f64>,
    phase: Phase,
    probe: Option<Probe>,
    /// Intervals to let the speed settle after a change.
    settle: u32,
    /// Intervals at a steady level.
    stable: u32,
    /// Reference speed of the steady level.
    steady: Option<f64>,
    /// Intervals left while waiting for a server.
    wait_left: u32,
    wait: u32,
    /// Times errors came with two or more files.
    crowded_errors: u32,
}

impl Tuner {
    /// Starts at `level` files at a time.
    pub fn new(level: usize, profile: Profile) -> Self {
        Tuner {
            profile,
            level: level.clamp(MIN_PARALLELISM, MAX_PARALLELISM),
            average: None,
            phase: Phase::SlowStart,
            probe: None,
            settle: 1,
            stable: 0,
            steady: None,
            wait_left: 0,
            wait: FIRST_WAIT,
            crowded_errors: 0,
        }
    }

    /// The profile in use.
    pub fn profile(&self) -> Profile {
        self.profile
    }

    /// The current level (0 while waiting for the server).
    pub fn level(&self) -> usize {
        self.level
    }

    /// Follows a level set by hand or from outside.
    pub fn reset(&mut self, level: usize) {
        *self = Tuner::new(level, self.profile);
    }

    /// Takes the bytes per second of the last interval, the retried errors
    /// in it and the limit of files for the servers of the job, and returns
    /// what to do next.
    pub fn next(&mut self, bytes_per_second: f64, retries: u32, limit: usize) -> Decision {
        let ceiling = self.profile.max_level().min(limit).clamp(MIN_PARALLELISM, MAX_PARALLELISM);
        let mut decision = Decision { level: self.level, reason: None, suggest_limit: None };

        if self.level == 0 {
            self.wait_left = self.wait_left.saturating_sub(1);
            if self.wait_left == 0 {
                self.level = 1;
                self.average = None;
                self.probe = None;
                self.settle = 1;
                decision.reason = Some("trying the server again".into());
            } else {
                decision.reason = None;
            }
            decision.level = self.level;
            return decision;
        }

        if retries > 0 {
            self.average = None;
            self.probe = None;
            self.phase = Phase::Steady;
            self.stable = 0;
            self.steady = None;
            self.settle = 1;
            if self.level > 1 {
                let lowered = ((self.level as f64 * self.profile.decrease()) as usize)
                    .clamp(MIN_PARALLELISM, self.level - 1);
                self.crowded_errors += 1;
                if self.crowded_errors >= LIMIT_AFTER_ERRORS {
                    decision.suggest_limit = Some(lowered);
                }
                decision.reason = Some(format!("lowered to {lowered}: {retries} errors"));
                self.level = lowered;
            } else {
                self.level = 0;
                self.wait_left = self.wait;
                decision.reason =
                    Some(format!("server not answering, waiting {} s", self.wait * 3));
                self.wait = (self.wait * 2).min(LONGEST_WAIT);
            }
            decision.level = self.level;
            return decision;
        }

        if self.level > ceiling {
            self.level = ceiling;
            self.probe = None;
            self.phase = Phase::Steady;
            self.steady = None;
            self.settle = 1;
            decision.reason = Some(format!("limited to {ceiling} by the server"));
            decision.level = self.level;
            return decision;
        }
        if bytes_per_second <= 0.0 {
            return decision; // nothing moved (planning, a pause): no information
        }
        let average = match self.average {
            Some(old) => 0.5 * bytes_per_second + 0.5 * old,
            None => bytes_per_second,
        };
        self.average = Some(average);
        self.wait = FIRST_WAIT; // the server answers: forget the waits
        if self.settle > 0 {
            self.settle -= 1;
            return decision;
        }
        let threshold = self.profile.threshold();

        if let Some(probe) = self.probe.take() {
            let gain = (average - probe.base) / probe.base;
            if gain > threshold {
                decision.reason =
                    Some(format!("{} files: {:+.0}% faster", self.level, gain * 100.0));
                if self.phase == Phase::Steady {
                    self.stable = self.profile.hold(); // it paid: try again soon
                }
                self.steady = Some(average);
            } else {
                self.level = probe.previous;
                self.phase = Phase::Steady;
                self.stable = 0;
                self.steady = Some(probe.base);
                self.settle = 1;
                decision.reason = Some(if gain < -threshold {
                    format!("kept at {}: more files were slower", self.level)
                } else {
                    format!("kept at {}: more files did not help", self.level)
                });
            }
            decision.level = self.level;
            return decision;
        }

        match self.phase {
            Phase::SlowStart => {
                let next = (self.level * 2).min(ceiling);
                if next > self.level {
                    self.begin_probe(next, average);
                } else {
                    self.phase = Phase::Steady;
                    self.steady = Some(average);
                }
            }
            Phase::Steady => {
                let reference = *self.steady.get_or_insert(average);
                if average < reference * 0.7 && self.level > 1 {
                    // The link got slower, not because of us: use less.
                    self.level -= 1;
                    self.steady = Some(average);
                    self.stable = 0;
                    self.settle = 1;
                    decision.reason = Some(format!("lowered to {}: speed dropped", self.level));
                } else {
                    self.steady = Some(0.9 * reference + 0.1 * average);
                    self.stable += 1;
                    if self.stable > self.profile.hold() && self.level < ceiling {
                        self.stable = 0;
                        let next = self.level + 1;
                        self.begin_probe(next, average);
                    }
                }
            }
        }
        decision.level = self.level;
        decision
    }

    fn begin_probe(&mut self, next: usize, base: f64) {
        self.probe = Some(Probe { previous: self.level, base });
        self.level = next;
        self.settle = 1;
    }
}

impl fmt::Display for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Profile::Balanced => "balanced",
            Profile::MaxSpeed => "max speed",
            Profile::Discreet => "discreet",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FREE: usize = MAX_PARALLELISM;

    /// A link of `capacity` bytes/s shared by the files, where each file can
    /// do at most `per_file`.
    fn link(level: usize, capacity: f64, per_file: f64) -> f64 {
        (level as f64 * per_file).min(capacity)
    }

    fn run(tuner: &mut Tuner, ticks: usize, capacity: f64, per_file: f64) {
        for _ in 0..ticks {
            let speed = link(tuner.level(), capacity, per_file);
            tuner.next(speed, 0, FREE);
        }
    }

    #[test]
    fn climbs_while_files_help_and_settles_when_the_link_is_full() {
        let mut tuner = Tuner::new(2, Profile::Balanced);
        run(&mut tuner, 40, 40.0, 10.0);
        assert!((4..=5).contains(&tuner.level()), "level {}", tuner.level());
    }

    #[test]
    fn one_file_is_enough_when_it_fills_the_link() {
        let mut tuner = Tuner::new(2, Profile::Balanced);
        run(&mut tuner, 40, 10.0, 10.0);
        assert!(tuner.level() <= 3, "level {}", tuner.level());
    }

    #[test]
    fn profiles_set_the_ceiling() {
        let mut max = Tuner::new(2, Profile::MaxSpeed);
        run(&mut max, 60, 1000.0, 10.0);
        let mut discreet = Tuner::new(1, Profile::Discreet);
        run(&mut discreet, 60, 1000.0, 10.0);
        assert!(max.level() > 8, "max speed went to {}", max.level());
        assert_eq!(discreet.level(), 3);
        let mut balanced = Tuner::new(2, Profile::Balanced);
        run(&mut balanced, 60, 1000.0, 10.0);
        assert_eq!(balanced.level(), 8);
    }

    #[test]
    fn errors_cut_the_level_and_suggest_a_limit_when_repeated() {
        let mut tuner = Tuner::new(8, Profile::Balanced);
        let first = tuner.next(100.0, 2, FREE);
        assert_eq!(first.level, 5);
        assert_eq!(first.suggest_limit, None);
        let second = tuner.next(100.0, 1, FREE);
        assert_eq!(second.level, 3);
        assert_eq!(second.suggest_limit, Some(3));
    }

    #[test]
    fn errors_at_one_file_wait_and_then_try_again_with_longer_waits() {
        let mut tuner = Tuner::new(1, Profile::Balanced);
        let d = tuner.next(0.0, 1, FREE);
        assert_eq!(d.level, 0);
        assert!(d.reason.unwrap().contains("waiting"));
        assert_eq!(tuner.next(0.0, 0, FREE).level, 0);
        let back = tuner.next(0.0, 0, FREE);
        assert_eq!(back.level, 1);
        // failing again waits twice as long
        assert_eq!(tuner.next(0.0, 1, FREE).level, 0);
        for _ in 0..3 {
            assert_eq!(tuner.next(0.0, 0, FREE).level, 0);
        }
        assert_eq!(tuner.next(0.0, 0, FREE).level, 1);
    }

    #[test]
    fn never_goes_above_the_server_limit() {
        let mut tuner = Tuner::new(6, Profile::MaxSpeed);
        let d = tuner.next(50.0, 0, 2);
        assert_eq!(d.level, 2);
        for _ in 0..30 {
            let speed = link(tuner.level(), 1000.0, 10.0);
            assert!(tuner.next(speed, 0, 2).level <= 2);
        }
    }

    #[test]
    fn follows_a_link_that_gets_slower() {
        let mut tuner = Tuner::new(2, Profile::Balanced);
        run(&mut tuner, 40, 1000.0, 10.0);
        let before = tuner.level();
        assert!(before >= 6);
        // the link is suddenly shared: everything is 4 times slower
        run(&mut tuner, 30, 1000.0, 2.0);
        assert!(tuner.level() <= before, "level {} after {before}", tuner.level());
    }

    #[test]
    fn idle_intervals_change_nothing() {
        let mut tuner = Tuner::new(3, Profile::Balanced);
        let d = tuner.next(0.0, 0, FREE);
        assert_eq!(d.level, 3);
        assert_eq!(d.reason, None);
    }
}
