//! Detection result types.
//!
//! The model is deliberately richer than loc-rs's `DetectionResult` enum:
//! a [`Detection`] carries not just *which* agent was found, but *why*
//! (which signal matched), *how confident* we are, and *when* the detection
//! happened.  This metadata is what makes downstream OTel analytics useful
//! — you can slice by confidence to filter out low-signal matches, or by
//! source kind to compare direct vs propagated detection rates.

use std::time::SystemTime;

use crate::pattern::EnvPattern;
use crate::registry::{AgentHarness, AgentHarnessKey, HarnessFamily};

// ─── Confidence ───────────────────────────────────────────────────────────────

/// How trustworthy is this detection?
///
/// Maps directly onto an OTel attribute value, so downstream queries can
/// filter (`WHERE confidence = 'high'`) when computing billing or quota
/// numbers, while still collecting low-confidence signals for trend analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Confidence {
    /// A dedicated harness-specific marker matched.
    ///
    /// Example: `CLAUDE_CODE=1` set in the process environment.  These
    /// signals are intentional and harness-specific, so a false positive is
    /// highly unlikely.
    High,
    /// A standard channel (`AI_AGENT` / `AGENT` env var) carried a value
    /// that maps to a known harness ID, OR a detection was propagated from
    /// a trusted CLI via the `x-agentdetect-agent` header.
    ///
    /// These signals are intentional but harness-agnostic (standard env
    /// var) or indirect (propagated), so they're slightly weaker evidence
    /// than a dedicated marker.
    Medium,
    /// A standard channel carried a value we don't recognise.
    ///
    /// The caller has explicitly self-identified, but we can't map it onto a
    /// known harness.  Useful for spotting new harnesses early — query OTel
    /// for `confidence = 'low' AND agent.id = 'unknown'` to discover
    /// emerging tools.
    Low,
}

impl Confidence {
    /// Canonical lowercase string ID, suitable for OTel attribute values.
    #[inline]
    pub const fn id(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }

    /// Parse a string ID back into a [`Confidence`].
    ///
    /// Returns `None` for unrecognised strings.  Used by the propagation
    /// reader to reconstruct a `Confidence` from the
    /// `x-agentdetect-confidence` header value.
    pub const fn from_id(id: &str) -> Option<Self> {
        match id.as_bytes() {
            b"high" => Some(Self::High),
            b"medium" => Some(Self::Medium),
            b"low" => Some(Self::Low),
            _ => None,
        }
    }
}

impl core::fmt::Display for Confidence {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.id())
    }
}

// ─── Source kind ──────────────────────────────────────────────────────────────

/// Which detection surface produced this result?
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceKind {
    /// Direct detection from a process environment variable.
    ///
    /// This is the only "real" detection surface — a harness spawning a
    /// shell sets an env var on the process tree, and we read it directly.
    EnvVar,
    /// Reconstructed from a propagated header on the API side.
    ///
    /// The CLI detected the agent via env vars, then wrote the identity
    /// onto its outgoing request via the `x-agentdetect-agent` header (see
    /// [`crate::propagation`]).  The API's middleware read that header and
    /// reconstructed this `Detection`.  This is NOT independent detection
    /// — it's trusted forwarding of a detection that happened elsewhere.
    Propagated,
}

impl SourceKind {
    /// Canonical lowercase string ID, suitable for OTel attribute values.
    #[inline]
    pub const fn id(self) -> &'static str {
        match self {
            Self::EnvVar => "env-var",
            Self::Propagated => "propagated",
        }
    }
}

impl core::fmt::Display for SourceKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.id())
    }
}

// ─── Raw signal (the evidence trail) ──────────────────────────────────────────

/// A single piece of evidence that contributed to a [`Detection`].
///
/// Every `Detection` carries at least one `RawSignal` so downstream
/// consumers can audit *why* a request was classified as agent X.  This is
/// critical when you're billing or rate-limiting based on detection — you
/// need to be able to explain "we counted this as Claude Code because the
/// `CLAUDE_CODE=1` env var was set in the process environment".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawSignal {
    /// A harness-specific env-var matched.
    HarnessEnvVar {
        /// Harness key this signal is associated with.
        key: AgentHarnessKey,
        /// Variable name (e.g. `"CLAUDE_CODE"`).
        name: &'static str,
        /// Variable value observed at detection time.
        value: String,
        /// Pattern that matched the value.
        pattern: EnvPattern,
    },
    /// A standard env-var (`AI_AGENT`, `AGENT`) carried a harness ID.
    StandardEnvVar {
        /// Variable name (e.g. `"AI_AGENT"`).
        name: &'static str,
        /// Raw value observed.
        value: String,
        /// Harness ID parsed out of the value, if any.
        resolved_key: Option<AgentHarnessKey>,
    },
    /// A detection propagated from a trusted CLI via the
    /// `x-agentdetect-agent` header.
    ///
    /// Produced by [`crate::propagation::read`] on the API side.  Not a
    /// direct detection — the actual detection happened in the CLI process
    /// (via env vars) and was forwarded here.
    Propagated {
        /// Harness ID carried in the `x-agentdetect-agent` header.
        agent_id: String,
        /// Confidence carried in the `x-agentdetect-confidence` header
        /// (defaults to `Medium` when absent).
        confidence: Confidence,
    },
}

impl RawSignal {
    /// Which surface did this signal come from?
    pub fn source_kind(&self) -> SourceKind {
        match self {
            Self::HarnessEnvVar { .. } | Self::StandardEnvVar { .. } => SourceKind::EnvVar,
            Self::Propagated { .. } => SourceKind::Propagated,
        }
    }

    /// Harness key this signal points at, if any.
    ///
    /// `None` for standard signals whose value we couldn't resolve to a
    /// known harness, and for propagated signals (which carry an ID string,
    /// not a key — use `AgentHarnessKey::from_id` to resolve).
    pub fn resolved_key(&self) -> Option<AgentHarnessKey> {
        match self {
            Self::HarnessEnvVar { key, .. } => Some(*key),
            Self::StandardEnvVar { resolved_key, .. } => *resolved_key,
            Self::Propagated { agent_id, .. } => AgentHarnessKey::from_id(agent_id),
        }
    }

    /// Variable / header name for OTel emission.
    pub fn signal_name(&self) -> &'static str {
        match self {
            Self::HarnessEnvVar { name, .. } => name,
            Self::StandardEnvVar { name, .. } => name,
            Self::Propagated { .. } => "x-agentdetect-agent",
        }
    }
}

// ─── AgentInfo ────────────────────────────────────────────────────────────────

/// Static identity information about the detected agent.
///
/// All static fields are borrowed from the registry — `AgentInfo` itself is
/// cheap to clone (only the optional `version` is heap-allocated).
#[derive(Debug, Clone)]
pub struct AgentInfo {
    /// Compile-time key for cross-referencing the registry.
    pub key: AgentHarnessKey,
    /// Canonical string ID (e.g. `"claude-code"`).
    pub id: &'static str,
    /// Human-readable label (e.g. `"Claude Code"`).
    pub pretty_label: &'static str,
    /// Vendor / origin family.
    pub family: HarnessFamily,
    /// URL to the harness's source repository, if public.
    pub repo_url: Option<&'static str>,
    /// URL to the harness's documentation or website.
    pub docs_url: Option<&'static str>,
    /// Short prose description of the harness.
    pub description: Option<&'static str>,
    /// Version string, if known.
    ///
    /// Most env-var detections don't carry a version (the env var is just
    /// `=1`), so this is usually `None`.  Populated only when a version
    /// signal is observed (e.g. propagated from a CLI that extracted it).
    pub version: Option<String>,
}

impl AgentInfo {
    /// Build an `AgentInfo` from a registry entry + optional version.
    pub(crate) fn from_registry(key: AgentHarnessKey, version: Option<String>) -> Self {
        let info: &AgentHarness = key.info();
        Self {
            key,
            id: key.id(),
            pretty_label: info.pretty_label,
            family: info.family,
            repo_url: info.repo_url,
            docs_url: info.docs_url,
            description: info.description,
            version,
        }
    }

    /// Convenience accessor: is this agent in the Anthropic family?
    #[inline]
    pub fn is_anthropic(&self) -> bool {
        self.family == HarnessFamily::Anthropic
    }

    /// Convenience accessor: is this agent in the OpenAI family?
    #[inline]
    pub fn is_openai(&self) -> bool {
        self.family == HarnessFamily::OpenAI
    }

    /// Convenience accessor: is this agent in the Google family?
    #[inline]
    pub fn is_google(&self) -> bool {
        self.family == HarnessFamily::Google
    }

    /// Convenience accessor: is this agent in the GitHub family?
    #[inline]
    pub fn is_github(&self) -> bool {
        self.family == HarnessFamily::GitHub
    }

    /// Convenience accessor: is this agent in the ByteDance family?
    #[inline]
    pub fn is_bytedance(&self) -> bool {
        self.family == HarnessFamily::ByteDance
    }

    /// Convenience accessor: is this agent in the Cognition family?
    #[inline]
    pub fn is_cognition(&self) -> bool {
        self.family == HarnessFamily::Cognition
    }

    /// Convenience accessor: is this agent in the Charm family?
    #[inline]
    pub fn is_charm(&self) -> bool {
        self.family == HarnessFamily::Charm
    }

    /// Convenience accessor: is this agent in the Cursor family?
    #[inline]
    pub fn is_cursor(&self) -> bool {
        self.family == HarnessFamily::Cursor
    }

    /// Convenience accessor: is this agent in the Block family?
    #[inline]
    pub fn is_block(&self) -> bool {
        self.family == HarnessFamily::Block
    }

    /// Convenience accessor: is this agent in the Replit family?
    #[inline]
    pub fn is_replit(&self) -> bool {
        self.family == HarnessFamily::Replit
    }

    /// Convenience accessor: is this agent in the AWS family?
    #[inline]
    pub fn is_aws(&self) -> bool {
        self.family == HarnessFamily::AWS
    }

    /// Convenience accessor: is this agent in the Nous Research family?
    #[inline]
    pub fn is_nous_research(&self) -> bool {
        self.family == HarnessFamily::NousResearch
    }

    /// Convenience accessor: is this agent in the Warp family?
    #[inline]
    pub fn is_warp(&self) -> bool {
        self.family == HarnessFamily::Warp
    }

    /// Convenience accessor: is this agent in the Zed family?
    #[inline]
    pub fn is_zed(&self) -> bool {
        self.family == HarnessFamily::Zed
    }

    /// Convenience accessor: is this agent in the Augment family?
    #[inline]
    pub fn is_augment(&self) -> bool {
        self.family == HarnessFamily::Augment
    }

    /// Convenience accessor: is this agent an unaffiliated community project?
    #[inline]
    pub fn is_community(&self) -> bool {
        self.family == HarnessFamily::Community
    }

    /// Convenience accessor: does this agent fall outside every dedicated
    /// family (including the [`AgentHarnessKey::Unknown`] sentinel)?
    #[inline]
    pub fn is_other(&self) -> bool {
        self.family == HarnessFamily::Other
    }
}

// ─── Detection ────────────────────────────────────────────────────────────────

/// The full result of an agent-harness detection attempt.
///
/// Produced by [`crate::detect_from_env()`] (direct env-var detection) or
/// [`crate::propagation::read`] (reconstructed from a propagated header on
/// the API side).  Pass it to
/// [`crate::otel::enrich_span`] / [`crate::otel::record_request`] to emit
/// OTel signals.
#[derive(Debug, Clone)]
pub struct Detection {
    /// Identity of the detected agent.
    pub agent: AgentInfo,
    /// How confident we are in this classification.
    pub confidence: Confidence,
    /// Primary signal that drove the detection.
    ///
    /// When multiple signals match (rare — usually only one env var is set
    /// per harness), the strongest one is stored here as the headline.  All
    /// matching signals (including the primary) are also collected in
    /// [`raw_signals`](Self::raw_signals).
    pub primary_signal: RawSignal,
    /// Every signal that matched during detection, in scan order.
    ///
    /// Always contains at least `primary_signal`.  Useful for debugging
    /// "why did this request get classified as X?" and for downstream
    /// analytics on multi-signal detections.
    pub raw_signals: Vec<RawSignal>,
    /// When the detection was performed.
    ///
    /// Wall-clock time from [`SystemTime::now`].  OTel pipelines typically
    /// carry their own timestamps, so this is mostly a fallback for
    /// offline consumers of the `Detection` struct.
    pub detected_at: SystemTime,
}

impl Detection {
    /// Canonical agent ID for OTel attributes / log lines.
    ///
    /// Convenience: `detection.agent_id()` instead of `detection.agent.id`.
    #[inline]
    pub fn agent_id(&self) -> &'static str {
        self.agent.id
    }

    /// Canonical agent label for OTel attributes / log lines.
    #[inline]
    pub fn agent_label(&self) -> &'static str {
        self.agent.pretty_label
    }

    /// Family ID for OTel attributes.
    #[inline]
    pub fn family_id(&self) -> &'static str {
        self.agent.family.id()
    }

    /// Which surface produced the primary signal?
    #[inline]
    pub fn source_kind(&self) -> SourceKind {
        self.primary_signal.source_kind()
    }

    /// Pretty-printed one-liner, e.g.
    /// `"claude-code (Claude Code) @ high via env-var CLAUDE_CODE"`.
    pub fn summary(&self) -> String {
        format!(
            "{} ({}) @ {} via {} {}",
            self.agent.id,
            self.agent.pretty_label,
            self.confidence.id(),
            self.source_kind().id(),
            self.primary_signal.signal_name(),
        )
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confidence_ids_are_stable() {
        assert_eq!(Confidence::High.id(), "high");
        assert_eq!(Confidence::Medium.id(), "medium");
        assert_eq!(Confidence::Low.id(), "low");
    }

    #[test]
    fn confidence_from_id_round_trips() {
        assert_eq!(Confidence::from_id("high"), Some(Confidence::High));
        assert_eq!(Confidence::from_id("medium"), Some(Confidence::Medium));
        assert_eq!(Confidence::from_id("low"), Some(Confidence::Low));
        assert_eq!(Confidence::from_id("bogus"), None);
        assert_eq!(Confidence::from_id(""), None);
    }

    #[test]
    fn source_kind_ids_are_stable() {
        assert_eq!(SourceKind::EnvVar.id(), "env-var");
        assert_eq!(SourceKind::Propagated.id(), "propagated");
    }

    #[test]
    fn agent_info_from_registry_carries_static_fields() {
        let info = AgentInfo::from_registry(AgentHarnessKey::ClaudeCode, None);
        assert_eq!(info.id, "claude-code");
        assert_eq!(info.pretty_label, "Claude Code");
        assert_eq!(info.family, HarnessFamily::Anthropic);
        assert!(info.is_anthropic());
        assert!(!info.is_openai());
        assert!(info.version.is_none());
    }

    #[test]
    fn all_family_convenience_methods_agree_with_family_field() {
        // Every is_*() method must return true for exactly its own family
        // and false for every other — for all 17 families, not just
        // Anthropic/OpenAI. Warp is used as the probe subject since it's
        // the harness whose family this test guards against regressing.
        let info = AgentInfo::from_registry(AgentHarnessKey::Warp, None);
        assert_eq!(info.family, HarnessFamily::Warp);
        assert!(info.is_warp());
        assert!(!info.is_anthropic());
        assert!(!info.is_openai());
        assert!(!info.is_google());
        assert!(!info.is_github());
        assert!(!info.is_bytedance());
        assert!(!info.is_cognition());
        assert!(!info.is_charm());
        assert!(!info.is_cursor());
        assert!(!info.is_block());
        assert!(!info.is_replit());
        assert!(!info.is_aws());
        assert!(!info.is_nous_research());
        assert!(!info.is_zed());
        assert!(!info.is_augment());
        assert!(!info.is_community());
        assert!(!info.is_other());
    }

    #[test]
    fn agent_info_carries_version_when_supplied() {
        let info = AgentInfo::from_registry(AgentHarnessKey::Codex, Some("1.0.0".to_string()));
        assert_eq!(info.version.as_deref(), Some("1.0.0"));
    }

    #[test]
    fn raw_signal_source_kind_for_each_variant() {
        let s1 = RawSignal::HarnessEnvVar {
            key: AgentHarnessKey::ClaudeCode,
            name: "CLAUDE_CODE",
            value: "1".into(),
            pattern: EnvPattern::Any,
        };
        assert_eq!(s1.source_kind(), SourceKind::EnvVar);
        assert_eq!(s1.resolved_key(), Some(AgentHarnessKey::ClaudeCode));
        assert_eq!(s1.signal_name(), "CLAUDE_CODE");

        let s2 = RawSignal::StandardEnvVar {
            name: "AI_AGENT",
            value: "claude-code".into(),
            resolved_key: Some(AgentHarnessKey::ClaudeCode),
        };
        assert_eq!(s2.source_kind(), SourceKind::EnvVar);
        assert_eq!(s2.resolved_key(), Some(AgentHarnessKey::ClaudeCode));

        let s3 = RawSignal::Propagated {
            agent_id: "codex".into(),
            confidence: Confidence::High,
        };
        assert_eq!(s3.source_kind(), SourceKind::Propagated);
        assert_eq!(s3.resolved_key(), Some(AgentHarnessKey::Codex));
        assert_eq!(s3.signal_name(), "x-agentdetect-agent");
    }
}
