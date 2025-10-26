mod indexeddb;
mod webcrypto;
mod clock;
mod api;

pub use api::*;

use wasm_bindgen::prelude::*;

// Set panic hook for better error messages in browser console
#[wasm_bindgen(start)]
pub fn start() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}
