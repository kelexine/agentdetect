//! Propagation: forward a detection from a CLI to an API.
//!
//! ## Why this exists
//!
//! agentdetect's core detection reads **process env vars** — and env vars
//! only exist locally in the CLI process.  When the CLI makes an outgoing
//! HTTP request to an API, the agent identity doesn't automatically travel
//! with it (a tool like `curl` or `gh` has no idea it's running under an
//! agent).
//!
//! The propagation layer solves this: the CLI uses [`inject`] to write the
//! detection onto its outgoing request via a header **we define and
//! control** (`x-agentdetect-agent`), and the API's middleware uses
//! [`read`] to reconstruct a [`Detection`] from that header.
//!
//! This is NOT third-party `User-Agent` sniffing.  The header is ours —
//! only code using agentdetect writes it, only code using agentdetect
//! reads it, and the API side trusts it because it trusts its own CLI.
//!
//! ## Header format
//!
//! | Header | Required | Example | Purpose |
//! |--------|----------|---------|---------|
//! | `x-agentdetect-agent` | yes | `claude-code` | Harness ID (looked up via [`AgentHarnessKey::from_id`]) |
//! | `x-agentdetect-confidence` | no | `high` | Original detection confidence (defaults to `medium`) |
//! | `x-agentdetect-version` | no | `1.0.7` | Harness version, if known |
//!
//! ## Round-trip
//!
//! ```text
//!  ┌──────────────┐    CLAUDE_CODE=1     ┌──────────────────┐
//!  │  CLI process │ ──────────────────▶ │  detect_from_env  │
//!  └──────┬───────┘                      │  → Detection      │
//!         │                              └────────┬──────────┘
//!         │                                       │ inject()
//!         │                                       ▼
//!         │                              ┌────────────────────┐
//!         │  POST /v1/call               │ x-agentdetect-agent│
//!         ├─────────────────────────────▶│ : claude-code      │
//!         │  (HTTP request)              │ x-agentdetect-conf │
//!         │                              │ : high             │
//!         │                              └─────────┬──────────┘
//!         │                                        │
//!         │                              ┌─────────▼──────────┐
//!         │                              │  API middleware    │
//!         │                              │  read() → Detection│
//!         │                              │  → otel::enrich…   │
//!         │                              └────────────────────┘
//! ```

use http::HeaderMap;

use crate::detection::{AgentInfo, Confidence, Detection, RawSignal};
use crate::registry::AgentHarnessKey;

// ─── Header name constants ────────────────────────────────────────────────────

/// Header carrying the harness ID (e.g. `"claude-code"`).
///
/// Required for a valid propagation.  Looked up via
/// [`AgentHarnessKey::from_id`] on the API side.
pub const HEADER_AGENT: &str = "x-agentdetect-agent";

/// Header carrying the original detection confidence (e.g. `"high"`).
///
/// Optional — defaults to [`Confidence::Medium`] when absent.  Parsed via
/// [`Confidence::from_id`].
pub const HEADER_CONFIDENCE: &str = "x-agentdetect-confidence";

/// Header carrying the harness version string (e.g. `"1.0.7"`).
///
/// Optional — most env-var detections don't carry a version, so this is
/// usually absent.
pub const HEADER_VERSION: &str = "x-agentdetect-version";

// ─── Inject (CLI side) ────────────────────────────────────────────────────────

/// Write the detection onto an outgoing request's headers.
///
/// Call this in your CLI code right before sending an HTTP request to your
/// API:
///
/// ```no_run
/// # #[cfg(all(feature = "http", feature = "otel"))] {
/// # use agentdetect::detect_from_env;
/// # use agentdetect::propagation::inject;
/// # use http::HeaderMap;
/// # let mut headers = HeaderMap::new();
/// if let Some(d) = detect_from_env() {
///     inject(&d, &mut headers);
/// }
/// // ... send request with `headers` ...
/// # }
/// ```
pub fn inject(detection: &Detection, headers: &mut HeaderMap) {
    // Agent ID (required).
    if let Ok(val) = http::HeaderValue::from_str(detection.agent.id) {
        headers.insert(HEADER_AGENT, val);
    }

    // Confidence (optional but always set — we know it).
    if let Ok(val) = http::HeaderValue::from_str(detection.confidence.id()) {
        headers.insert(HEADER_CONFIDENCE, val);
    }

    // Version (optional — usually absent for env-var detections).
    if let Some(version) = &detection.agent.version {
        if let Ok(val) = http::HeaderValue::from_str(version) {
            headers.insert(HEADER_VERSION, val);
        }
    }
}

/// Build a `Vec<(name, value)>` of propagation headers without a `HeaderMap`.
///
/// Useful when you're using a non-`http` HTTP client (e.g. `reqwest`'s
/// builder API) and want to set headers one at a time.
pub fn header_pairs(detection: &Detection) -> Vec<(&'static str, String)> {
    let mut out = Vec::with_capacity(3);
    out.push((HEADER_AGENT, detection.agent.id.to_string()));
    out.push((HEADER_CONFIDENCE, detection.confidence.id().to_string()));
    if let Some(version) = &detection.agent.version {
        out.push((HEADER_VERSION, version.clone()));
    }
    out
}

// ─── Read (API side) ──────────────────────────────────────────────────────────

/// Reconstruct a [`Detection`] from propagated headers.
///
/// # Behavior
///
/// - **Header absent** → returns `None` (undetected request — no propagation
///   happened).
/// - **Header present, recognised harness ID** (e.g. `"claude-code"`) →
///   returns `Some(Detection)` with the resolved [`AgentHarnessKey`],
///   [`SourceKind::Propagated`], and confidence from the
///   `x-agentdetect-confidence` header (defaults to [`Confidence::Medium`]
///   when absent).
/// - **Header present, unrecognised harness ID** (e.g.
///   `"future-tool-2027"`) → returns `Some(Detection)` with
///   [`AgentHarnessKey::Unknown`] and [`Confidence::Low`], matching the
///   env-var behavior for unrecognised `AI_AGENT` values.  The raw
///   agent-id string is preserved in [`RawSignal::Propagated`] so OTel
///   consumers can discover emerging tools.
///
/// Used by `agentdetect-tower`'s middleware — you usually don't call this
/// directly unless you're writing your own middleware.
///
/// [`SourceKind::Propagated`]: crate::detection::SourceKind::Propagated
pub fn read(headers: &HeaderMap) -> Option<Detection> {
    let agent_id = headers.get(HEADER_AGENT)?.to_str().ok()?;
    if agent_id.is_empty() {
        return None;
    }

    let (key, confidence) = match AgentHarnessKey::from_id(agent_id) {
        Some(k) => {
            // Recognised harness — trust the propagated confidence (default Medium).
            let conf = headers
                .get(HEADER_CONFIDENCE)
                .and_then(|v| v.to_str().ok())
                .and_then(Confidence::from_id)
                .unwrap_or(Confidence::Medium);
            (k, conf)
        }
        None => {
            // Unrecognised harness — match env-var behavior: Unknown / Low.
            // The raw agent_id string is preserved in the Propagated signal
            // so OTel consumers can spot emerging tools.
            (AgentHarnessKey::Unknown, Confidence::Low)
        }
    };

    let version = headers
        .get(HEADER_VERSION)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let primary_signal = RawSignal::Propagated {
        agent_id: agent_id.to_string(),
        confidence,
    };

    Some(Detection {
        agent: AgentInfo::from_registry(key, version),
        confidence,
        primary_signal: primary_signal.clone(),
        raw_signals: vec![primary_signal],
        detected_at: std::time::SystemTime::now(),
    })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::EnvSource;
    use crate::detect_from_env_with;
    use crate::detection::SourceKind;
    use std::collections::HashMap;

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

    fn sample_detection() -> Detection {
        let env = FakeEnv::default().set("CLAUDE_CODE", "1");
        detect_from_env_with(&env).expect("detection should fire")
    }

    #[test]
    fn inject_then_read_round_trips() {
        let original = sample_detection();
        let mut headers = HeaderMap::new();
        inject(&original, &mut headers);

        let reconstructed = read(&headers).expect("should reconstruct");
        assert_eq!(reconstructed.agent.id, "claude-code");
        assert_eq!(reconstructed.agent.key, AgentHarnessKey::ClaudeCode);
        assert_eq!(reconstructed.confidence, original.confidence);
        assert_eq!(reconstructed.source_kind(), SourceKind::Propagated);
    }

    #[test]
    fn read_returns_none_when_header_absent() {
        let headers = HeaderMap::new();
        assert!(read(&headers).is_none());
    }

    #[test]
    fn read_returns_unknown_low_for_unrecognised_agent_id() {
        // Unrecognised harness ID → matches env-var behavior: Unknown / Low.
        // The raw agent_id string is preserved in the Propagated signal.
        let mut headers = HeaderMap::new();
        headers.insert(
            HEADER_AGENT,
            http::HeaderValue::from_static("future-tool-2027"),
        );
        // Even if the header claims high confidence, an unrecognised agent
        // must be Low.
        headers.insert(HEADER_CONFIDENCE, http::HeaderValue::from_static("high"));
        let d = read(&headers).expect("should still produce a Detection");
        assert_eq!(d.agent.id, "unknown");
        assert_eq!(d.agent.key, AgentHarnessKey::Unknown);
        assert_eq!(d.confidence, Confidence::Low);
        assert_eq!(d.source_kind(), SourceKind::Propagated);
        // Raw agent_id preserved in the signal for OTel visibility.
        match &d.primary_signal {
            RawSignal::Propagated { agent_id, .. } => {
                assert_eq!(agent_id, "future-tool-2027");
            }
            _ => panic!("expected Propagated signal"),
        }
    }

    #[test]
    fn read_defaults_confidence_to_medium_when_absent() {
        let mut headers = HeaderMap::new();
        headers.insert(HEADER_AGENT, http::HeaderValue::from_static("codex"));
        let d = read(&headers).expect("should reconstruct");
        assert_eq!(d.confidence, Confidence::Medium);
    }

    #[test]
    fn read_carries_version_when_present() {
        let mut headers = HeaderMap::new();
        headers.insert(HEADER_AGENT, http::HeaderValue::from_static("codex"));
        headers.insert(HEADER_VERSION, http::HeaderValue::from_static("1.2.0"));
        let d = read(&headers).expect("should reconstruct");
        assert_eq!(d.agent.version.as_deref(), Some("1.2.0"));
    }

    #[test]
    fn header_pairs_returns_agent_and_confidence() {
        let d = sample_detection();
        let pairs = header_pairs(&d);
        assert!(
            pairs
                .iter()
                .any(|(k, v)| *k == HEADER_AGENT && v == "claude-code")
        );
        assert!(
            pairs
                .iter()
                .any(|(k, v)| *k == HEADER_CONFIDENCE && v == "high")
        );
    }

    #[test]
    fn header_pairs_omits_version_when_absent() {
        let d = sample_detection();
        let pairs = header_pairs(&d);
        assert!(!pairs.iter().any(|(k, _)| *k == HEADER_VERSION));
    }
}
