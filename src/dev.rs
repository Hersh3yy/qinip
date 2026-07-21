use std::fmt::Display;
use std::sync::OnceLock;

static ENABLED: OnceLock<bool> = OnceLock::new();

pub fn enabled() -> bool {
    *ENABLED.get_or_init(|| {
        let Ok(value) = std::env::var("QINIP_DEV").or_else(|_| std::env::var("DJ_PLAYER_DEV")) else {
            return false;
        };
        value == "1" || value.eq_ignore_ascii_case("true")
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
