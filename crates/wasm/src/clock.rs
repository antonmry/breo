use pds_core::{Clock as ClockTrait};
use wasm_bindgen::prelude::*;

/// Clock implementation using JS Date
pub struct Clock;

impl Clock {
    pub fn new() -> Self {
        Self
    }
}

impl ClockTrait for Clock {
    fn now(&self) -> u64 {
        (js_sys::Date::now() as u64)
    }
}
