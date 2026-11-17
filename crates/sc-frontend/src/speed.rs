//! Transfer speed and time left, measured over the last few seconds.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How far back the speed looks.
const WINDOW: Duration = Duration::from_secs(5);

/// Needs at least this much history before it shows a speed.
const MIN_SPAN: Duration = Duration::from_secs(1);

/// Bytes moved, with the time they were reported.
#[derive(Debug, Default)]
pub struct SpeedMeter {
    samples: VecDeque<(Instant, u64)>,
}

impl SpeedMeter {
    /// Notes that `bytes` were just transferred.
    pub fn record(&mut self, now: Instant, bytes: u64) {
        self.samples.push_back((now, bytes));
        self.forget_old(now);
    }

    /// Forgets everything (the transfers ended).
    pub fn clear(&mut self) {
        self.samples.clear();
    }

    fn forget_old(&mut self, now: Instant) {
        while self.samples.front().is_some_and(|(t, _)| now.duration_since(*t) > WINDOW) {
            self.samples.pop_front();
        }
    }

    /// Bytes per second over the last few seconds, once there is enough
    /// history to say.
    pub fn speed(&self, now: Instant) -> Option<f64> {
        let recent: Vec<&(Instant, u64)> =
            self.samples.iter().filter(|(t, _)| now.duration_since(*t) <= WINDOW).collect();
        let first = recent.first()?.0;
        let span = now.duration_since(first);
        if span < MIN_SPAN {
            return None;
        }
        let bytes: u64 = recent.iter().map(|(_, b)| b).sum();
        Some(bytes as f64 / span.as_secs_f64())
    }
}

/// `0:42`, or `1:02:03` from an hour on.
pub fn clock(seconds: u64) -> String {
    let (h, m, s) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_needs_a_second_of_history_and_forgets_the_old() {
        let start = Instant::now();
        let at = |s: u64| start + Duration::from_secs(s);
        let mut meter = SpeedMeter::default();
        meter.record(at(0), 1000);
        assert_eq!(meter.speed(at(0)), None, "too early");
        meter.record(at(2), 3000);
        assert_eq!(meter.speed(at(2)), Some(2000.0), "4000 bytes in 2 s");
        // Ten seconds later only the new samples count.
        meter.record(at(12), 500);
        meter.record(at(14), 500);
        assert_eq!(meter.speed(at(14)), Some(500.0));
        meter.clear();
        assert_eq!(meter.speed(at(14)), None);
    }

    #[test]
    fn clock_text() {
        assert_eq!(clock(42), "0:42");
        assert_eq!(clock(125), "2:05");
        assert_eq!(clock(3723), "1:02:03");
    }
}
