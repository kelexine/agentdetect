//! Detection engine.
//!
//! One entry point: [`detect_from_env`] (and its `_with` variant for test
//! injection).  Returns [`Option<Detection>`] (`None` when no signal is
//! found).
//!
//! # Detection priority
//!
//! 1. **Harness-specific env-vars first.**  A dedicated marker (e.g.
//!    `CLAUDE_CODE=1`) is stronger evidence than a generic `AI_AGENT`
//!    value, so we scan the registry in priority order before consulting
//!    standard channels.
//! 2. **Standard env-vars second.**  `AI_AGENT` / `AGENT` carry a harness
//!    ID directly.  A recognised ID becomes a `Medium`-confidence
//!    detection; an unrecognised ID becomes a `Low`-confidence detection
//!    whose `agent.id` is `"unknown"`.
//! 3. If nothing matched, returns `None`.
//!
//! # The bit-flip primitive
//!
//! For the canonical use case (switch output format based on whether an
//! agent is active), use [`is_agent`] — it returns `true` if any known
//! harness env var is set, without building a full `Detection`.  This is
//! the cheapest way to answer "should I emit pretty output or
//! machine-readable output?"

use std::time::SystemTime;

use crate::detection::{AgentInfo, Confidence, Detection, RawSignal};
use crate::registry::{AGENT_HARNESSES, AgentHarnessKey, STANDARD_AGENT_ENV_VARS};

// ─── Env-source abstraction ───────────────────────────────────────────────────

/// Read-only view over a process environment, used by [`detect_from_env`].
///
/// Default implementation reads `std::env::var`.  Tests inject a fake
/// implementation so they don't mutate global process state.
pub trait EnvSource {
    /// Returns the value of the named env var, or `None` if unset.
    fn get(&self, name: &str) -> Option<String>;
}

/// Default [`EnvSource`] that reads from the live process environment.
#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessEnv;

impl EnvSource for ProcessEnv {
    fn get(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }
}

// ─── The bit-flip primitive ───────────────────────────────────────────────────

/// Returns `true` if any known agent harness is active in the current
/// process environment.
///
/// This is the **bit-flip primitive** — the cheapest way to answer "should
/// I emit human-readable output or machine-readable output?":
///
/// ```no_run
/// if agentdetect::is_agent() {
///     // Agent harness is active — emit machine-readable output.
///     println!("# Agent-Detected: {}", agentdetect::detect().unwrap().agent.id);
///     // ... TSV data ...
/// } else {
///     // Human terminal — emit pretty output.
///     // ... coloured table ...
/// }
/// ```
///
/// Equivalent to `detect_from_env().is_some()` but avoids building the
/// full [`Detection`] struct when the caller only needs the boolean.
pub fn is_agent() -> bool {
    is_agent_with(&ProcessEnv)
}

/// `is_agent` with a custom [`EnvSource`].  Useful in tests.
pub fn is_agent_with(env: &dyn EnvSource) -> bool {
    // Phase 1: harness-specific env-vars.
    for (_, harness) in AGENT_HARNESSES {
        for check in harness.env_vars {
            if let Some(val) = env.get(check.name) {
                if check.pattern.matches(&val) {
                    return true;
                }
            }
        }
    }
    // Phase 2: standard env-vars.
    for &var in STANDARD_AGENT_ENV_VARS {
        if let Some(val) = env.get(var) {
            if !val.is_empty() {
                return true;
            }
        }
    }
    false
}

// ─── Full detection ───────────────────────────────────────────────────────────

/// Detect the active agent harness from the current process environment.
///
/// Convenience wrapper around [`detect_from_env_with`] using [`ProcessEnv`].
/// Returns `None` if no signal is found.
///
/// # Example
///
/// ```no_run
/// # use agentdetect::detect_from_env;
/// if let Some(d) = detect_from_env() {
///     eprintln!("agent: {} ({})", d.agent.id, d.agent.pretty_label);
/// }
/// ```
pub fn detect_from_env() -> Option<Detection> {
    detect_from_env_with(&ProcessEnv)
}

/// Alias for [`detect_from_env`] — shorter name for the common case.
pub fn detect() -> Option<Detection> {
    detect_from_env()
}

/// Detect using a custom [`EnvSource`].  Useful in tests or when the host
/// process exposes env access through a non-`std::env` channel (e.g. WASI).
pub fn detect_from_env_with(env: &dyn EnvSource) -> Option<Detection> {
    let mut all_signals: Vec<RawSignal> = Vec::new();

    // ── Phase 1: harness-specific env-vars (priority order) ──────────────
    for (key, harness) in AGENT_HARNESSES {
        for check in harness.env_vars {
            if let Some(val) = env.get(check.name) {
                if check.pattern.matches(&val) {
                    all_signals.push(RawSignal::HarnessEnvVar {
                        key: *key,
                        name: check.name,
                        value: val,
                        pattern: check.pattern,
                    });
                    // First harness-specific match wins for the headline.
                    break;
                }
            }
        }
        if !all_signals.is_empty() {
            break;
        }
    }

    // ── Phase 2: standard env-vars (fallback) ────────────────────────────
    if all_signals.is_empty() {
        for &var in STANDARD_AGENT_ENV_VARS {
            if let Some(val) = env.get(var) {
                if val.is_empty() {
                    continue;
                }
                let resolved_key = AgentHarnessKey::from_id(&val);
                all_signals.push(RawSignal::StandardEnvVar {
                    name: var,
                    value: val,
                    resolved_key,
                });
                break;
            }
        }
    }

    if all_signals.is_empty() {
        return None;
    }

    build_detection(all_signals)
}

// ─── Detection builder ────────────────────────────────────────────────────────

/// Promote the first signal in `signals` into a full [`Detection`].
///
/// Assumes `signals` is non-empty (caller checks).  Confidence is derived
/// from the primary signal:
///
/// - Harness-specific match → `High`.
/// - Standard signal with a recognised harness ID → `Medium`.
/// - Standard signal with an unrecognised ID → `Low`, and the agent uses
///   [`AgentHarnessKey::Unknown`].
fn build_detection(mut signals: Vec<RawSignal>) -> Option<Detection> {
    if signals.is_empty() {
        return None;
    }

    // Primary is the first signal; we keep it as the headline and also
    // leave it in raw_signals (for the evidence trail).
    let primary = signals.remove(0);
    let mut all_signals = Vec::with_capacity(signals.len() + 1);
    all_signals.push(primary.clone());
    all_signals.extend(signals);

    let (key, confidence) = match &primary {
        RawSignal::HarnessEnvVar {
            key,
            value: _,
            pattern: _,
            ..
        } => (*key, Confidence::High),
        RawSignal::StandardEnvVar {
            resolved_key: Some(k),
            ..
        } => (*k, Confidence::Medium),
        RawSignal::StandardEnvVar { value: _, .. } => {
            // Unrecognised standard signal — use the real Unknown sentinel.
            return Some(Detection {
                agent: AgentInfo::from_registry(AgentHarnessKey::Unknown, None),
                confidence: Confidence::Low,
                primary_signal: primary,
                raw_signals: all_signals,
                detected_at: SystemTime::now(),
            });
        }
        // Propagated signals are produced by `propagation::read`, not by
        // this module.  If one reaches here it's a programming error.
        RawSignal::Propagated { .. } => return None,
    };

    Some(Detection {
        agent: AgentInfo::from_registry(key, None),
        confidence,
        primary_signal: primary,
        raw_signals: all_signals,
        detected_at: SystemTime::now(),
    })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detection::SourceKind;
    use std::collections::HashMap;

    // ── Test helpers ────────────────────────────────────────────────────────

    /// In-memory EnvSource for tests — does not touch the live process env.
    #[derive(Default)]
    struct FakeEnv(HashMap<&'static str, String>);

    impl FakeEnv {
        fn set(mut self, k: &'static str, v: impl Into<String>) -> Self {
            self.0.insert(k, v.into());
            self
        }
    }

    impl EnvSource for FakeEnv {
        fn get(&self, name: &str) -> Option<String> {
            self.0
                .iter()
                .find_map(|(k, v)| (*k == name).then_some(v.clone()))
        }
    }

    // ── is_agent ────────────────────────────────────────────────────────────

    #[test]
    fn is_agent_returns_false_when_nothing_set() {
        let env = FakeEnv::default();
        assert!(!is_agent_with(&env));
    }

    #[test]
    fn is_agent_returns_true_for_harness_specific_var() {
        let env = FakeEnv::default().set("CLAUDE_CODE", "1");
        assert!(is_agent_with(&env));
    }

    #[test]
    fn is_agent_returns_true_for_standard_var_known() {
        let env = FakeEnv::default().set("AI_AGENT", "codex");
        assert!(is_agent_with(&env));
    }

    #[test]
    fn is_agent_returns_true_for_standard_var_unknown() {
        let env = FakeEnv::default().set("AI_AGENT", "mystery-tool");
        assert!(is_agent_with(&env));
    }

    #[test]
    fn is_agent_returns_false_for_empty_standard_var() {
        let env = FakeEnv::default().set("AI_AGENT", "");
        assert!(!is_agent_with(&env));
    }

    // ── detect_from_env ─────────────────────────────────────────────────────

    #[test]
    fn detect_known_harness_via_specific_var() {
        let env = FakeEnv::default().set("CLAUDE_CODE", "1");
        let d = detect_from_env_with(&env).expect("claude-code should be detected");
        assert_eq!(d.agent.id, "claude-code");
        assert_eq!(d.confidence, Confidence::High);
        assert_eq!(d.source_kind(), SourceKind::EnvVar);
        assert!(!d.raw_signals.is_empty());
    }

    #[test]
    fn detect_known_harness_via_standard_var() {
        let env = FakeEnv::default().set("AI_AGENT", "codex");
        let d = detect_from_env_with(&env).expect("codex should be detected via AI_AGENT");
        assert_eq!(d.agent.id, "codex");
        assert_eq!(d.confidence, Confidence::Medium);
    }

    #[test]
    fn detect_unknown_harness_via_standard_var() {
        let env = FakeEnv::default().set("AI_AGENT", "my-obscure-agent");
        let d = detect_from_env_with(&env).expect("unknown agent should still produce a Detection");
        assert_eq!(d.agent.id, "unknown");
        assert_eq!(d.agent.key, AgentHarnessKey::Unknown);
        assert_ne!(d.agent.key, AgentHarnessKey::Devin);
        assert_eq!(d.confidence, Confidence::Low);
    }

    #[test]
    fn detect_specific_var_beats_standard_var_conflict() {
        // AI_AGENT=codex + CRUSH=1 → should return Crush (specific beats standard).
        let env = FakeEnv::default()
            .set("AI_AGENT", "codex")
            .set("CRUSH", "1");
        let d = detect_from_env_with(&env).expect("detection should fire");
        assert_eq!(d.agent.id, "crush");
        assert_eq!(d.confidence, Confidence::High);
    }

    #[test]
    fn detect_cowork_wins_over_claude_code_when_both_set() {
        let env = FakeEnv::default()
            .set("CLAUDE_CODE_IS_COWORK", "1")
            .set("CLAUDE_CODE", "1");
        let d = detect_from_env_with(&env).expect("cowork should win");
        assert_eq!(d.agent.id, "cowork");
    }

    #[test]
    fn detect_returns_none_when_nothing_set() {
        let env = FakeEnv::default();
        assert!(detect_from_env_with(&env).is_none());
    }

    #[test]
    fn detect_warp_requires_exact_match() {
        let env = FakeEnv::default().set("TERM_PROGRAM", "iTerm.app");
        assert!(detect_from_env_with(&env).is_none());

        let env = FakeEnv::default().set("TERM_PROGRAM", "WarpTerminal");
        let d = detect_from_env_with(&env).expect("warp should match on exact value");
        assert_eq!(d.agent.id, "warp");
    }
}
