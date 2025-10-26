mod indexeddb;
mod webcrypto;
mod clock;
mod api;

pub use api::*;

use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}
