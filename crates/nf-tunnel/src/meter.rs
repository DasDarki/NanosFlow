use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::time::Instant;

use crate::Traffic;

pub struct TrafficMeter {
    sent: AtomicU64,
    received: AtomicU64,
    started: Instant,
    last_activity: AtomicU64,
}

impl Default for TrafficMeter {
    fn default() -> Self {
        Self {
            sent: AtomicU64::new(0),
            received: AtomicU64::new(0),
            started: Instant::now(),
            last_activity: AtomicU64::new(0),
        }
    }
}

impl TrafficMeter {
    fn touch(&self) {
        self.last_activity.store(self.started.elapsed().as_millis() as u64, Ordering::Relaxed);
    }

    pub fn add_sent(&self, length: usize) {
        self.sent.fetch_add(length as u64, Ordering::Relaxed);
        self.touch();
    }

    pub fn add_received(&self, length: usize) {
        self.received.fetch_add(length as u64, Ordering::Relaxed);
        self.touch();
    }

    pub fn idle_for(&self) -> Duration {
        self.started.elapsed().saturating_sub(Duration::from_millis(self.last_activity.load(Ordering::Relaxed)))
    }

    pub fn traffic(&self) -> Traffic {
        Traffic {
            sent: self.sent.load(Ordering::Relaxed),
            received: self.received.load(Ordering::Relaxed),
        }
    }
}
