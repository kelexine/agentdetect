//! Compile-time pattern matching for env-vars.
//!
//! [`EnvPattern`] mirrors the loc-rs convention: `Any` (set + non-empty),
//! `Exact`, `Prefix`. Used for harness-specific marker variables such as
//! `CLAUDE_CODE=1`.
//!
//! All matchers are `const fn` so the entire registry can live in the
//! binary's read-only data segment with zero heap allocation.

#![allow(dead_code)]

// ─── Env-var match pattern ────────────────────────────────────────────────────

/// Compile-time pattern for matching an environment variable's value.
///
/// Mirrors the loc-rs convention:
/// - `"*"`          → [`EnvPattern::Any`]
/// - `"<value>"`    → [`EnvPattern::Exact`]
/// - `"<prefix>*"`  → [`EnvPattern::Prefix`]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvPattern {
    /// Variable is set to any non-empty value.
    Any,
    /// Variable equals this exact string.
    Exact(&'static str),
    /// Variable value begins with this prefix.
    Prefix(&'static str),
}

impl EnvPattern {
    /// Returns `true` if `value` satisfies the pattern.
    #[inline]
    pub const fn matches(self, value: &str) -> bool {
        match self {
            Self::Any => !value.is_empty(),
            Self::Exact(expected) => const_str_eq(value, expected),
            Self::Prefix(prefix) => const_str_starts_with(value, prefix),
        }
    }
}

// ─── Const-fn string primitives ───────────────────────────────────────────────
//
// These exist because `&str::eq` and `&str::starts_with` are not yet
// `const fn` on stable Rust as of 1.96.  When they stabilise, these
// helpers can be deleted and the call-sites inlined.

#[inline]
const fn const_str_eq(a: &str, b: &str) -> bool {
    let a = a.as_bytes();
    let b = b.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

#[inline]
const fn const_str_starts_with(haystack: &str, prefix: &str) -> bool {
    let h = haystack.as_bytes();
    let p = prefix.as_bytes();
    if h.len() < p.len() {
        return false;
    }
    let mut i = 0;
    while i < p.len() {
        if h[i] != p[i] {
            return false;
        }
        i += 1;
    }
    true
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_pattern_any_matches_nonempty() {
        assert!(EnvPattern::Any.matches("1"));
        assert!(EnvPattern::Any.matches("true"));
    }

    #[test]
    fn env_pattern_any_rejects_empty() {
        assert!(!EnvPattern::Any.matches(""));
    }

    #[test]
    fn env_pattern_exact_matches() {
        assert!(EnvPattern::Exact("WarpTerminal").matches("WarpTerminal"));
    }

    #[test]
    fn env_pattern_exact_rejects_mismatch() {
        assert!(!EnvPattern::Exact("WarpTerminal").matches("xterm"));
        assert!(!EnvPattern::Exact("WarpTerminal").matches(""));
    }

    #[test]
    fn env_pattern_prefix_matches() {
        assert!(EnvPattern::Prefix("v1.").matches("v1.2.3"));
        assert!(EnvPattern::Prefix("v1.").matches("v1."));
    }

    #[test]
    fn env_pattern_prefix_rejects_too_short() {
        assert!(!EnvPattern::Prefix("v1.").matches("v1"));
        assert!(!EnvPattern::Prefix("v1.").matches(""));
    }

    #[test]
    fn env_pattern_matches_is_const_evaluable() {
        const MATCHES: bool = EnvPattern::Exact("WarpTerminal").matches("WarpTerminal");
        const {
            assert!(MATCHES);
        }
    }
}
