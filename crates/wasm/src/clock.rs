//! Clock implementation using JavaScript Date

use chrono::{DateTime, Utc};
use pds_core::traits::Clock;

/// Clock implementation using JavaScript Date.now()
#[derive(Debug, Clone, Default)]
pub struct JsClock;

impl JsClock {
    /// Create a new JavaScript clock
    pub fn new() -> Self {
        Self
    }
}

impl Clock for JsClock {
    fn now(&self) -> DateTime<Utc> {
        // Use JavaScript Date.now() for accurate browser time
        let millis = js_sys::Date::now();
        let secs = (millis / 1000.0) as i64;
        let nanos = ((millis % 1000.0) * 1_000_000.0) as u32;
        
        DateTime::from_timestamp(secs, nanos)
            .unwrap_or_else(|| Utc::now())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_arch = "wasm32")]
    fn test_js_clock_creation() {
        let clock = JsClock::new();
        let now = clock.now();
        assert!(now.timestamp() > 0);
    }
}
