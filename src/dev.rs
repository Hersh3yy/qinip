use std::fmt::Display;
use std::sync::OnceLock;

static ENABLED: OnceLock<bool> = OnceLock::new();

pub fn enabled() -> bool {
    *ENABLED.get_or_init(|| {
        std::env::var("DJ_PLAYER_DEV")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    })
}

pub fn log(msg: impl Display) {
    if enabled() {
        eprintln!("[dev] {msg}");
    }
}

pub fn log_timing(label: &str, start: std::time::Instant) {
    if enabled() {
        eprintln!("[dev] {label} took {:.2?}", start.elapsed());
    }
}
