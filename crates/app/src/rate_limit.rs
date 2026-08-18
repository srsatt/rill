use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Clone, Default)]
pub struct AttemptLimiter {
    entries: Arc<Mutex<HashMap<String, Entry>>>,
}

struct Entry {
    started: Instant,
    attempts: u32,
}

impl AttemptLimiter {
    pub fn attempt(&self, key: &str, maximum: u32, window: Duration) -> bool {
        let Ok(mut entries) = self.entries.lock() else {
            return false;
        };
        let now = Instant::now();
        entries.retain(|_, entry| now.duration_since(entry.started) < window);
        let entry = entries.entry(key.to_owned()).or_insert(Entry {
            started: now,
            attempts: 0,
        });
        if entry.attempts >= maximum {
            return false;
        }
        entry.attempts += 1;
        true
    }

    pub fn clear(&self, key: &str) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.remove(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_after_limit_and_can_reset() {
        let limiter = AttemptLimiter::default();
        assert!(limiter.attempt("client", 2, Duration::from_secs(60)));
        assert!(limiter.attempt("client", 2, Duration::from_secs(60)));
        assert!(!limiter.attempt("client", 2, Duration::from_secs(60)));
        limiter.clear("client");
        assert!(limiter.attempt("client", 2, Duration::from_secs(60)));
    }
}
