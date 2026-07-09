//! Integration test: end-to-end detection scenarios that exercise the
//! public API the way a downstream consumer would.

use std::collections::HashMap;

use agentdetect::detect::EnvSource;
use agentdetect::{
    AgentHarnessKey, Confidence, HarnessFamily, SourceKind, detect_from_env_with, is_agent_with,
};

// ── Test doubles ──────────────────────────────────────────────────────────────

#[derive(Default)]
struct MapEnv(HashMap<&'static str, String>);
impl MapEnv {
    fn set(mut self, k: &'static str, v: impl Into<String>) -> Self {
        self.0.insert(k, v.into());
        self
    }
}
impl EnvSource for MapEnv {
    fn get(&self, name: &str) -> Option<String> {
        self.0
            .iter()
            .find_map(|(k, v)| (*k == name).then_some(v.clone()))
    }
}

// ── is_agent (the bit-flip primitive) ─────────────────────────────────────────

#[test]
fn is_agent_false_when_nothing_set() {
    let env = MapEnv::default();
    assert!(!is_agent_with(&env));
}

#[test]
fn is_agent_true_for_harness_specific_var() {
    let env = MapEnv::default().set("CLAUDE_CODE", "1");
    assert!(is_agent_with(&env));
}

#[test]
fn is_agent_true_for_standard_var_known() {
    let env = MapEnv::default().set("AI_AGENT", "codex");
    assert!(is_agent_with(&env));
}

#[test]
fn is_agent_true_for_standard_var_unknown() {
    let env = MapEnv::default().set("AI_AGENT", "mystery-tool");
    assert!(is_agent_with(&env));
}

#[test]
fn is_agent_false_for_empty_standard_var() {
    let env = MapEnv::default().set("AI_AGENT", "");
    assert!(!is_agent_with(&env));
}

// ── detect_from_env ───────────────────────────────────────────────────────────

#[test]
fn end_to_end_claude_code_via_env_marker() {
    let env = MapEnv::default().set("CLAUDE_CODE", "1");
    let d = detect_from_env_with(&env).expect("claude-code should be detected");
    assert_eq!(d.agent.id, "claude-code");
    assert_eq!(d.agent.pretty_label, "Claude Code");
    assert_eq!(d.agent.family, HarnessFamily::Anthropic);
    assert_eq!(d.confidence, Confidence::High);
    assert_eq!(d.source_kind(), SourceKind::EnvVar);
    assert!(!d.raw_signals.is_empty());
}

#[test]
fn end_to_end_priority_specific_marker_beats_standard_var() {
    // Both CLAUDE_CODE=1 and AI_AGENT=codex are set — the specific CLAUDE_CODE
    // marker must win.
    let env = MapEnv::default()
        .set("CLAUDE_CODE", "1")
        .set("AI_AGENT", "codex");
    let d = detect_from_env_with(&env).expect("detection should fire");
    assert_eq!(d.agent.id, "claude-code");
    assert_eq!(d.confidence, Confidence::High);
}

#[test]
fn end_to_end_priority_cowork_beats_claude_code() {
    let env = MapEnv::default()
        .set("CLAUDE_CODE_IS_COWORK", "1")
        .set("CLAUDE_CODE", "1");
    let d = detect_from_env_with(&env).expect("cowork should win");
    assert_eq!(d.agent.id, "cowork");
}

#[test]
fn end_to_end_priority_cursor_cli_beats_cursor() {
    let env = MapEnv::default()
        .set("CURSOR_AGENT", "1")
        .set("CURSOR_TRACE_ID", "abc");
    let d = detect_from_env_with(&env).expect("cursor-cli should win");
    assert_eq!(d.agent.id, "cursor-cli");
}

#[test]
fn end_to_end_warp_requires_exact_term_program_match() {
    // iTerm.app must NOT match the Warp pattern (which is Exact("WarpTerminal")).
    let env = MapEnv::default().set("TERM_PROGRAM", "iTerm.app");
    assert!(detect_from_env_with(&env).is_none());

    let env = MapEnv::default().set("TERM_PROGRAM", "WarpTerminal");
    let d = detect_from_env_with(&env).expect("warp exact match should fire");
    assert_eq!(d.agent.id, "warp");
}

#[test]
fn end_to_end_unknown_standard_signal_returns_low_confidence() {
    let env = MapEnv::default().set("AI_AGENT", "future-agent-2027");
    let d = detect_from_env_with(&env).expect("unknown signal should still produce a Detection");
    assert_eq!(d.agent.id, "unknown");
    assert_eq!(d.agent.pretty_label, "Unknown");
    assert_eq!(d.agent.key, AgentHarnessKey::Unknown);
    assert_ne!(d.agent.key, AgentHarnessKey::Devin);
    assert_eq!(d.confidence, Confidence::Low);
    assert_eq!(d.source_kind(), SourceKind::EnvVar);
}

#[test]
fn end_to_end_no_signal_returns_none() {
    let env = MapEnv::default();
    assert!(detect_from_env_with(&env).is_none());
}

// ── Registry coverage ─────────────────────────────────────────────────────────

#[test]
fn registry_has_23_harnesses() {
    assert_eq!(agentdetect::REGISTRY_SIZE, 23);
    assert_eq!(AgentHarnessKey::ALL.len(), 23);
}

#[test]
fn every_harness_round_trips_through_id() {
    for &key in AgentHarnessKey::ALL {
        let id = key.id();
        assert_eq!(
            AgentHarnessKey::from_id(id),
            Some(key),
            "round-trip failed for {id}"
        );
    }
}

#[test]
fn known_agent_ids_are_stable() {
    // Lock down the canonical IDs — downstream OTel queries depend on these
    // strings never changing.
    assert_eq!(AgentHarnessKey::ClaudeCode.id(), "claude-code");
    assert_eq!(AgentHarnessKey::Codex.id(), "codex");
    assert_eq!(AgentHarnessKey::Cursor.id(), "cursor");
    assert_eq!(AgentHarnessKey::CursorCli.id(), "cursor-cli");
    assert_eq!(AgentHarnessKey::GeminiCli.id(), "gemini-cli");
    assert_eq!(AgentHarnessKey::GithubCopilot.id(), "github-copilot");
    assert_eq!(AgentHarnessKey::Devin.id(), "devin");
    assert_eq!(AgentHarnessKey::Cowork.id(), "cowork");
}

// ── Propagation round-trip (feature-gated) ────────────────────────────────────

#[cfg(feature = "http")]
mod propagation_tests {
    use super::*;
    use agentdetect::propagation;

    fn sample_detection() -> agentdetect::Detection {
        let env = MapEnv::default().set("CLAUDE_CODE", "1");
        detect_from_env_with(&env).expect("detection should fire")
    }

    #[test]
    fn propagation_round_trip_preserves_agent_and_confidence() {
        let original = sample_detection();
        let mut headers = http::HeaderMap::new();
        propagation::inject(&original, &mut headers);

        let reconstructed = propagation::read(&headers).expect("should reconstruct");
        assert_eq!(reconstructed.agent.id, "claude-code");
        assert_eq!(reconstructed.agent.key, AgentHarnessKey::ClaudeCode);
        assert_eq!(reconstructed.confidence, original.confidence);
        assert_eq!(reconstructed.source_kind(), SourceKind::Propagated);
        assert_eq!(reconstructed.source_kind(), SourceKind::Propagated);
    }

    #[test]
    fn propagation_read_returns_none_when_header_absent() {
        let headers = http::HeaderMap::new();
        assert!(propagation::read(&headers).is_none());
    }

    #[test]
    fn propagation_read_returns_unknown_low_for_unrecognised_agent_id() {
        let mut headers = http::HeaderMap::new();
        headers.insert(
            propagation::HEADER_AGENT,
            http::HeaderValue::from_static("future-tool-2027"),
        );
        let d = propagation::read(&headers).expect("should still produce a Detection");
        assert_eq!(d.agent.id, "unknown");
        assert_eq!(d.agent.key, AgentHarnessKey::Unknown);
        assert_eq!(d.confidence, Confidence::Low);
        assert_eq!(d.source_kind(), SourceKind::Propagated);
    }

    #[test]
    fn propagation_source_kind_is_propagated_not_envvar() {
        let original = sample_detection();
        let mut headers = http::HeaderMap::new();
        propagation::inject(&original, &mut headers);

        let reconstructed = propagation::read(&headers).expect("should reconstruct");
        // The original was env-var; the reconstructed is propagated.
        assert_eq!(original.source_kind(), SourceKind::EnvVar);
        assert_eq!(reconstructed.source_kind(), SourceKind::Propagated);
    }
}
