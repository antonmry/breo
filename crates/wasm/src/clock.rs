use pds_core::traits::Clock;
use chrono::{DateTime, Utc};

/// Clock implementation using JS Date
#[derive(Debug, Clone, Default)]
pub struct JsClock;

impl JsClock {
    pub fn new() -> Self {
        Self
    }
}

impl Clock for JsClock {
    fn now(&self) -> DateTime<Utc> {
        let timestamp_ms = js_sys::Date::now();
        let secs = (timestamp_ms / 1000.0) as i64;
        let nsecs = ((timestamp_ms % 1000.0) * 1_000_000.0) as u32;
        DateTime::from_timestamp(secs, nsecs).unwrap_or_else(|| Utc::now())
    }
}
