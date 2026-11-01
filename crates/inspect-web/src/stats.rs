//! Frame-time statistics over a sliding window: the FPS readout's mean and
//! 95th percentile of the frame time over the last two seconds. Pure, so it
//! is tested natively.

use std::collections::VecDeque;

/// Frame times (ms) with the time (ms) each was taken, over a window.
#[derive(Clone, Debug)]
pub struct FrameStats {
    window_ms: f64,
    samples: VecDeque<(f64, f64)>,
}

impl FrameStats {
    /// Statistics over the last `window_ms` ms.
    pub fn new(window_ms: f64) -> Self {
        Self {
            window_ms,
            samples: VecDeque::new(),
        }
    }

    /// Record a frame of `dt` ms ending at time `now` ms.
    pub fn push(&mut self, now: f64, dt: f64) {
        self.samples.push_back((now, dt));
        while self
            .samples
            .front()
            .is_some_and(|&(t, _)| t < now - self.window_ms)
        {
            self.samples.pop_front();
        }
    }

    /// How many frames are in the window.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// The mean frame time, ms.
    pub fn mean(&self) -> Option<f64> {
        (!self.samples.is_empty())
            .then(|| self.samples.iter().map(|s| s.1).sum::<f64>() / self.samples.len() as f64)
    }

    /// The 95th percentile frame time, ms (nearest rank).
    pub fn p95(&self) -> Option<f64> {
        let mut v: Vec<f64> = self.samples.iter().map(|s| s.1).collect();
        if v.is_empty() {
            return None;
        }
        v.sort_by(f64::total_cmp);
        let rank = (0.95 * v.len() as f64).ceil() as usize;
        Some(v[rank.clamp(1, v.len()) - 1])
    }

    /// A one-line readout: frames per second from the mean, and both times.
    pub fn readout(&self) -> String {
        match (self.mean(), self.p95()) {
            (Some(m), Some(p)) => format!("{:.0} fps  mean {m:.1} ms  p95 {p:.1} ms", 1000.0 / m),
            _ => "no frames".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_forgets_old_frames_and_p95_is_nearest_rank() {
        let mut s = FrameStats::new(2000.0);
        assert_eq!(s.mean(), None);
        // 100 frames of 16 ms, then one slow 50 ms frame.
        for i in 0..100 {
            s.push(i as f64 * 16.0, 16.0);
        }
        s.push(1700.0, 50.0);
        assert_eq!(s.len(), 101);
        assert_eq!(s.p95(), Some(16.0));
        assert!((s.mean().unwrap() - 16.34).abs() < 0.01);
        // 10 slow frames of 100 make the 95th percentile slow.
        for i in 0..10 {
            s.push(1800.0 + i as f64, 100.0);
        }
        assert_eq!(s.p95(), Some(100.0));
        // Two seconds on, only recent frames remain.
        s.push(3900.0, 20.0);
        assert!(s.len() < 15);
    }
}
