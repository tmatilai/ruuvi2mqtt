//! Pure-Rust modules with no ESP-IDF dependencies, so their unit tests run
//! on the host (`make test`).

pub mod diagnostics;
pub mod led_pattern;
pub mod mac;
pub mod payload;
pub mod scan;
