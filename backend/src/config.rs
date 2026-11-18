//! Reading settings from the environment, one way everywhere.

/// A positive number from `key`, else `default` (unset, unparsable or ≤ 0).
pub fn env_num(key: &str, default: u64) -> u64 {
    std::env::var(key).ok().and_then(|v| v.trim().parse().ok()).filter(|v| *v > 0).unwrap_or(default)
}

/// A non-blank, trimmed value from `key`.
/// Compare secrets without leaking where they differ through timing.
pub fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub fn env_opt(key: &str) -> Option<String> {
    std::env::var(key).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// On for `1`, `true`, `on`, `yes` (any case); off otherwise.
pub fn env_flag(key: &str) -> bool {
    matches!(env_opt(key).map(|v| v.to_ascii_lowercase()).as_deref(), Some("1" | "true" | "on" | "yes"))
}

/// On unless set to `0`, `false`, `off` or `no` — for safety switches that default on.
pub fn env_flag_default_on(key: &str) -> bool {
    !matches!(env_opt(key).map(|v| v.to_ascii_lowercase()).as_deref(), Some("0" | "false" | "off" | "no"))
}
