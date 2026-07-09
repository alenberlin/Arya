//! A coarse per-user fixed-window rate limiter. It runs AFTER authentication
//! (keyed by the resolved user id), so a leaked-but-valid token can't drain the
//! wallet as fast as the upstream allows, and unauthenticated spam can't bloat
//! the map (those requests are rejected before they reach here).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct RateLimit {
    limit: u32,
    window: Duration,
    windows: Mutex<HashMap<String, (Instant, u32)>>,
}

impl RateLimit {
    pub fn new(limit: u32, window: Duration) -> Self {
        Self {
            limit,
            window,
            windows: Mutex::new(HashMap::new()),
        }
    }

    /// Returns true if a request under `key` is allowed, false if `key` has
    /// already used its `limit` for the current window.
    pub fn check(&self, key: &str) -> bool {
        self.check_at(key, Instant::now())
    }

    fn check_at(&self, key: &str, now: Instant) -> bool {
        let mut windows = self.windows.lock().expect("ratelimit lock");
        let entry = windows.entry(key.to_string()).or_insert((now, 0));
        if now.duration_since(entry.0) >= self.window {
            *entry = (now, 0);
        }
        if entry.1 >= self.limit {
            return false;
        }
        entry.1 += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_up_to_the_limit_then_blocks_within_a_window() {
        let rl = RateLimit::new(2, Duration::from_secs(60));
        let t0 = Instant::now();
        assert!(rl.check_at("u1", t0));
        assert!(rl.check_at("u1", t0));
        assert!(!rl.check_at("u1", t0), "3rd request in-window is blocked");
        // A different user has its own budget.
        assert!(rl.check_at("u2", t0));
    }

    #[test]
    fn resets_after_the_window_elapses() {
        let rl = RateLimit::new(1, Duration::from_secs(60));
        let t0 = Instant::now();
        assert!(rl.check_at("u1", t0));
        assert!(!rl.check_at("u1", t0));
        // Past the window, the count resets.
        assert!(rl.check_at("u1", t0 + Duration::from_secs(61)));
    }
}
