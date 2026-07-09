//! Static registry of AI agent harnesses.
//!
//! All data is resolved at compile time via `const`/`static` definitions.
//! [`EnvPattern`] encodes match semantics in the type system rather than
//! parsing strings at runtime.
//!
//! # Detection priority
//!
//! Detection priority follows insertion order in [`AGENT_HARNESSES`]; the
//! first matching entry wins.  Two ordering invariants are enforced by tests:
//!
//! - `Cowork` must precede `ClaudeCode` — when both `CLAUDE_CODE` and
//!   `CLAUDE_CODE_IS_COWORK` are set, the more-specific Cowork signal wins.
//! - `CursorCli` must precede `Cursor` — child processes inside Cursor's
//!   terminal inherit `CURSOR_TRACE_ID`, so `cursor` is a lower-priority
//!   fallback and `cursor-cli` (`CURSOR_AGENT`) is the more specific signal.
//!
//! # Env vars are the only surface
//!
//! agentdetect reads **only** the process environment.  A harness spawning
//! a shell sets an env var (`CLAUDE_CODE=1`, `CURSOR_AGENT=1`, etc.) on the
//! process tree it spawns — that is the *entire* signal.  HTTP headers and
//! JSON bodies are NOT sniffed: a tool like `curl` or `gh` running under an
//! agent has no idea it's under an agent, so its wire traffic carries no
//! agent-identifying signal.  See the crate-level docs for the full rationale.

#![allow(dead_code)]

use crate::pattern::EnvPattern;

// ─── Env-var check ────────────────────────────────────────────────────────────

/// A single env-var probe: "this variable, when set to a value matching
/// `pattern`, is evidence that harness X is running."
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnvVarCheck {
    /// Name of the environment variable to inspect.
    pub name: &'static str,
    /// Pattern the variable's value must satisfy.
    pub pattern: EnvPattern,
}

// ─── Harness family (for downstream analytics grouping) ───────────────────────

/// Vendor / origin family for a harness.
///
/// Lets downstream OTel queries group traffic by vendor ("what % of traffic
/// is Anthropic-origin?") without hard-coding harness IDs into the query.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HarnessFamily {
    Anthropic,
    OpenAI,
    Google,
    GitHub,
    ByteDance,
    Cognition,
    Charm,
    Cursor,
    Block,
    Replit,
    AWS,
    NousResearch,
    Community,
    Other,
}

impl HarnessFamily {
    /// Canonical lowercase string ID, suitable for OTel attribute values.
    #[inline]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::OpenAI => "openai",
            Self::Google => "google",
            Self::GitHub => "github",
            Self::ByteDance => "bytedance",
            Self::Cognition => "cognition",
            Self::Charm => "charm",
            Self::Cursor => "cursor",
            Self::Block => "block",
            Self::Replit => "replit",
            Self::AWS => "aws",
            Self::NousResearch => "nous-research",
            Self::Community => "community",
            Self::Other => "other",
        }
    }
}

impl core::fmt::Display for HarnessFamily {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.id())
    }
}

// ─── Harness descriptor ───────────────────────────────────────────────────────

/// Descriptor for a known AI agent harness.
///
/// All `&'static str` fields keep the struct entirely stack-resident and
/// zero-cost to copy — no heap allocation ever occurs for a detection.
#[derive(Debug, Clone, Copy)]
pub struct AgentHarness {
    /// Human-readable display name (e.g. used in dashboards).
    pub pretty_label: &'static str,
    /// URL to the harness's source repository, if public.
    pub repo_url: Option<&'static str>,
    /// URL to the harness's documentation or website.
    pub docs_url: Option<&'static str>,
    /// Short prose description of the harness.
    pub description: Option<&'static str>,
    /// Vendor / origin family — used for analytics grouping.
    pub family: HarnessFamily,
    /// Environment variable checks; detection succeeds on the **first** match.
    /// An empty slice means detection relies solely on
    /// [`STANDARD_AGENT_ENV_VARS`] (e.g. `devin`).
    pub env_vars: &'static [EnvVarCheck],
}

// ─── Harness key (compile-time enum) ─────────────────────────────────────────

/// Compile-time enumeration of every known agent harness ID, plus an
/// [`Unknown`](Self::Unknown) sentinel for unrecognised standard-channel
/// signals.
///
/// Each known variant is documented in the [`AGENT_HARNESSES`] registry
/// below — the enum itself exists to give each harness a stable `as u8`
/// discriminant for fast comparison and a `const fn` `id()` / `from_id()`
/// pair.  See the registry table for per-harness documentation.
///
/// [`Unknown`](Self::Unknown) is deliberately excluded from
/// [`Self::ALL`] and [`AGENT_HARNESSES`] — it is not a real harness, only
/// a sentinel used in [`crate::detection::AgentInfo`] when a standard
/// channel carries a value that doesn't map to any known harness.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentHarnessKey {
    Antigravity,
    AugmentCli,
    Cline,
    Cowork,
    ClaudeCode,
    Codex,
    Crush,
    GeminiCli,
    GithubCopilot,
    Goose,
    HermesAgent,
    KiloCode,
    Kiro,
    OpenClaw,
    OpenCode,
    Pi,
    Replit,
    Trae,
    Warp,
    Zed,
    CursorCli,
    Cursor,
    Devin,
    /// Sentinel for unrecognised standard-channel signals.
    ///
    /// Not a real harness — excluded from [`Self::ALL`] and
    /// [`AGENT_HARNESSES`].  Used as `AgentInfo::key` when `AI_AGENT` /
    /// `AGENT` carries a value that [`from_id`](Self::from_id) doesn't
    /// recognise.
    Unknown,
}

impl AgentHarnessKey {
    /// Canonical string ID for this key (the registry map key, OTel attribute value).
    #[inline]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Antigravity => "antigravity",
            Self::AugmentCli => "augment-cli",
            Self::Cline => "cline",
            Self::Cowork => "cowork",
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::Crush => "crush",
            Self::GeminiCli => "gemini-cli",
            Self::GithubCopilot => "github-copilot",
            Self::Goose => "goose",
            Self::HermesAgent => "hermes-agent",
            Self::KiloCode => "kilo-code",
            Self::Kiro => "kiro",
            Self::OpenClaw => "openclaw",
            Self::OpenCode => "opencode",
            Self::Pi => "pi",
            Self::Replit => "replit",
            Self::Trae => "trae",
            Self::Warp => "warp",
            Self::Zed => "zed",
            Self::CursorCli => "cursor-cli",
            Self::Cursor => "cursor",
            Self::Devin => "devin",
            Self::Unknown => "unknown",
        }
    }

    /// Parse a string ID into the corresponding key, or `None` if unrecognised.
    ///
    /// This is a `const fn` — the compiler will fold calls with literal
    /// arguments into a compile-time constant.
    #[inline]
    pub const fn from_id(id: &str) -> Option<Self> {
        match id.as_bytes() {
            b"antigravity" => Some(Self::Antigravity),
            b"augment-cli" => Some(Self::AugmentCli),
            b"cline" => Some(Self::Cline),
            b"cowork" => Some(Self::Cowork),
            b"claude-code" => Some(Self::ClaudeCode),
            b"codex" => Some(Self::Codex),
            b"crush" => Some(Self::Crush),
            b"gemini-cli" => Some(Self::GeminiCli),
            b"github-copilot" => Some(Self::GithubCopilot),
            b"goose" => Some(Self::Goose),
            b"hermes-agent" => Some(Self::HermesAgent),
            b"kilo-code" => Some(Self::KiloCode),
            b"kiro" => Some(Self::Kiro),
            b"openclaw" => Some(Self::OpenClaw),
            b"opencode" => Some(Self::OpenCode),
            b"pi" => Some(Self::Pi),
            b"replit" => Some(Self::Replit),
            b"trae" => Some(Self::Trae),
            b"warp" => Some(Self::Warp),
            b"zed" => Some(Self::Zed),
            b"cursor-cli" => Some(Self::CursorCli),
            b"cursor" => Some(Self::Cursor),
            b"devin" => Some(Self::Devin),
            // "unknown" is deliberately NOT mapped here.  Standard-channel
            // values that don't match a known harness must produce
            // Confidence::Low, not Confidence::Medium.  The Unknown variant
            // is only constructed internally by the detection engine.
            _ => None,
        }
    }

    /// Returns the static [`AgentHarness`] descriptor for this key.
    ///
    /// For [`Unknown`](Self::Unknown), returns a sentinel descriptor with
    /// placeholder fields (pretty_label = "Unknown", family = Other,
    /// empty env_vars).
    #[inline]
    pub const fn info(self) -> &'static AgentHarness {
        // Unknown is a sentinel, not a registry entry.
        if self as u8 == Self::Unknown as u8 {
            return &UNKNOWN_HARNESS_INFO;
        }
        let mut i = 0;
        while i < AGENT_HARNESSES.len() {
            if AGENT_HARNESSES[i].0 as u8 == self as u8 {
                return &AGENT_HARNESSES[i].1;
            }
            i += 1;
        }
        // INVARIANT: every known AgentHarnessKey variant must be present in
        // AGENT_HARNESSES.  Unknown is handled above.
        panic!("AgentHarnessKey variant missing from AGENT_HARNESSES");
    }

    /// All known harness keys, in detection-priority order.
    pub const ALL: &'static [AgentHarnessKey] = &[
        Self::Antigravity,
        Self::AugmentCli,
        Self::Cline,
        Self::Cowork,
        Self::ClaudeCode,
        Self::Codex,
        Self::Crush,
        Self::GeminiCli,
        Self::GithubCopilot,
        Self::Goose,
        Self::HermesAgent,
        Self::KiloCode,
        Self::Kiro,
        Self::OpenClaw,
        Self::OpenCode,
        Self::Pi,
        Self::Replit,
        Self::Trae,
        Self::Warp,
        Self::Zed,
        Self::CursorCli,
        Self::Cursor,
        Self::Devin,
    ];
}

impl core::fmt::Display for AgentHarnessKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.id())
    }
}

// ─── Standard signal channels ─────────────────────────────────────────────────

/// Standard environment variables that any tool can set to identify itself.
///
/// When one of these is set, its value is treated directly as an
/// [`AgentHarnessKey`] ID; unrecognised values are reported as `"unknown"`.
pub const STANDARD_AGENT_ENV_VARS: &[&str] = &["AI_AGENT", "AGENT"];

// ─── Unknown-harness sentinel ─────────────────────────────────────────────────

/// Static [`AgentHarness`] descriptor returned by
/// [`AgentHarnessKey::info`] when the key is [`AgentHarnessKey::Unknown`].
///
/// This is NOT a real harness — it is a placeholder used in
/// [`crate::detection::AgentInfo`] when a standard channel (`AI_AGENT`,
/// `AGENT`) carries a value that doesn't map to any known harness.  Detection
/// in that case is [`Confidence::Low`].
///
/// [`Confidence::Low`]: crate::detection::Confidence::Low
const UNKNOWN_HARNESS_INFO: AgentHarness = AgentHarness {
    pretty_label: "Unknown",
    repo_url: None,
    docs_url: None,
    description: Some("Unrecognised standard agent signal — value did not map to a known harness."),
    family: HarnessFamily::Other,
    env_vars: &[],
};

// ─── Registry ─────────────────────────────────────────────────────────────────

/// Ordered registry of all known agent harnesses.
///
/// **Insertion order determines detection priority**: harnesses are checked
/// top-to-bottom and the first match wins.  See the module docs for the two
/// ordering invariants that must be preserved.
pub const AGENT_HARNESSES: &[(AgentHarnessKey, AgentHarness)] = &[
    (
        AgentHarnessKey::Antigravity,
        AgentHarness {
            pretty_label: "Antigravity",
            repo_url: None,
            docs_url: Some("https://antigravity.google"),
            description: Some("Agentic development platform from Google built around Gemini."),
            family: HarnessFamily::Google,
            env_vars: &[EnvVarCheck {
                name: "ANTIGRAVITY_AGENT",
                pattern: EnvPattern::Any,
            }],
        },
    ),
    (
        AgentHarnessKey::AugmentCli,
        AgentHarness {
            pretty_label: "Augment CLI",
            repo_url: Some("https://github.com/augmentcode/auggie"),
            docs_url: Some("https://www.augmentcode.com"),
            description: Some("Auggie, the command-line coding agent from Augment Code."),
            family: HarnessFamily::Other,
            env_vars: &[EnvVarCheck {
                name: "AUGMENT_AGENT",
                pattern: EnvPattern::Any,
            }],
        },
    ),
    (
        AgentHarnessKey::Cline,
        AgentHarness {
            pretty_label: "Cline",
            repo_url: Some("https://github.com/cline/cline"),
            docs_url: Some("https://cline.bot"),
            description: Some("Open-source autonomous coding agent for VS Code."),
            family: HarnessFamily::Community,
            env_vars: &[EnvVarCheck {
                name: "CLINE_ACTIVE",
                pattern: EnvPattern::Any,
            }],
        },
    ),
    (
        // Must stay before `ClaudeCode`: when both `CLAUDE_CODE` and
        // `CLAUDE_CODE_IS_COWORK` are set, the more-specific Cowork signal wins.
        AgentHarnessKey::Cowork,
        AgentHarness {
            pretty_label: "Cowork",
            repo_url: None,
            docs_url: Some("https://claude.com/product/cowork"),
            description: Some(
                "Anthropic's agent for autonomous knowledge work, built on top of Claude Code.",
            ),
            family: HarnessFamily::Anthropic,
            env_vars: &[EnvVarCheck {
                name: "CLAUDE_CODE_IS_COWORK",
                pattern: EnvPattern::Any,
            }],
        },
    ),
    (
        AgentHarnessKey::ClaudeCode,
        AgentHarness {
            pretty_label: "Claude Code",
            repo_url: Some("https://github.com/anthropics/claude-code"),
            docs_url: Some("https://code.claude.com/docs"),
            description: Some("Anthropic's agentic coding tool that lives in your terminal."),
            family: HarnessFamily::Anthropic,
            env_vars: &[
                EnvVarCheck {
                    name: "CLAUDECODE",
                    pattern: EnvPattern::Any,
                },
                EnvVarCheck {
                    name: "CLAUDE_CODE",
                    pattern: EnvPattern::Any,
                },
            ],
        },
    ),
    (
        AgentHarnessKey::Codex,
        AgentHarness {
            pretty_label: "Codex",
            repo_url: Some("https://github.com/openai/codex"),
            docs_url: Some("https://developers.openai.com/codex"),
            description: Some("OpenAI's lightweight coding agent that runs in your terminal."),
            family: HarnessFamily::OpenAI,
            env_vars: &[
                EnvVarCheck {
                    name: "CODEX_SANDBOX",
                    pattern: EnvPattern::Any,
                },
                EnvVarCheck {
                    name: "CODEX_CI",
                    pattern: EnvPattern::Any,
                },
                EnvVarCheck {
                    name: "CODEX_THREAD_ID",
                    pattern: EnvPattern::Any,
                },
            ],
        },
    ),
    (
        AgentHarnessKey::Crush,
        AgentHarness {
            pretty_label: "Crush",
            repo_url: Some("https://github.com/charmbracelet/crush"),
            docs_url: Some("https://github.com/charmbracelet/crush"),
            description: Some("Charm's open-source AI coding agent for the terminal."),
            family: HarnessFamily::Charm,
            env_vars: &[EnvVarCheck {
                name: "CRUSH",
                pattern: EnvPattern::Any,
            }],
        },
    ),
    (
        AgentHarnessKey::GeminiCli,
        AgentHarness {
            pretty_label: "Gemini CLI",
            repo_url: Some("https://github.com/google-gemini/gemini-cli"),
            docs_url: Some("https://geminicli.com"),
            description: Some(
                "Google's open-source terminal AI coding agent powered by Gemini models.",
            ),
            family: HarnessFamily::Google,
            env_vars: &[EnvVarCheck {
                name: "GEMINI_CLI",
                pattern: EnvPattern::Any,
            }],
        },
    ),
    (
        AgentHarnessKey::GithubCopilot,
        AgentHarness {
            pretty_label: "GitHub Copilot",
            repo_url: None,
            docs_url: Some("https://docs.github.com/copilot"),
            description: Some("GitHub's AI coding assistant."),
            family: HarnessFamily::GitHub,
            env_vars: &[
                EnvVarCheck {
                    name: "COPILOT_MODEL",
                    pattern: EnvPattern::Any,
                },
                EnvVarCheck {
                    name: "COPILOT_ALLOW_ALL",
                    pattern: EnvPattern::Any,
                },
                EnvVarCheck {
                    name: "COPILOT_GITHUB_TOKEN",
                    pattern: EnvPattern::Any,
                },
            ],
        },
    ),
    (
        AgentHarnessKey::Goose,
        AgentHarness {
            pretty_label: "Goose",
            repo_url: Some("https://github.com/aaif-goose/goose"),
            docs_url: Some("https://goose-docs.ai/"),
            description: Some(
                "Open-source, extensible AI agent, originally from Block and now part of the Agentic AI Foundation.",
            ),
            family: HarnessFamily::Block,
            env_vars: &[EnvVarCheck {
                name: "GOOSE_TERMINAL",
                pattern: EnvPattern::Any,
            }],
        },
    ),
    (
        AgentHarnessKey::HermesAgent,
        AgentHarness {
            pretty_label: "Hermes Agent",
            repo_url: Some("https://github.com/NousResearch/hermes-agent"),
            docs_url: Some("https://hermes-agent.nousresearch.com/docs"),
            description: Some("Nous Research's self-improving, multi-provider terminal AI agent."),
            family: HarnessFamily::NousResearch,
            env_vars: &[EnvVarCheck {
                name: "HERMES_SESSION_ID",
                pattern: EnvPattern::Any,
            }],
        },
    ),
    (
        AgentHarnessKey::KiloCode,
        AgentHarness {
            pretty_label: "Kilo Code",
            repo_url: Some("https://github.com/Kilo-Org/kilocode"),
            docs_url: Some("https://kilocode.ai/docs"),
            description: Some(
                "Open-source agentic coding agent for VS Code, JetBrains, and the terminal.",
            ),
            family: HarnessFamily::Community,
            env_vars: &[EnvVarCheck {
                name: "KILOCODE_FEATURE",
                pattern: EnvPattern::Any,
            }],
        },
    ),
    (
        AgentHarnessKey::Kiro,
        AgentHarness {
            pretty_label: "Kiro",
            repo_url: None,
            docs_url: Some("https://kiro.dev"),
            description: Some("AWS's agentic IDE for spec-driven AI software development."),
            family: HarnessFamily::AWS,
            env_vars: &[EnvVarCheck {
                name: "AGENT_CONTEXT_OUT",
                pattern: EnvPattern::Any,
            }],
        },
    ),
    (
        AgentHarnessKey::OpenClaw,
        AgentHarness {
            pretty_label: "OpenClaw",
            repo_url: Some("https://github.com/openclaw/openclaw"),
            docs_url: Some("https://openclaw.ai"),
            description: Some(
                "Open-source, self-hosted personal AI assistant that runs on your own devices.",
            ),
            family: HarnessFamily::Community,
            env_vars: &[EnvVarCheck {
                name: "OPENCLAW_SHELL",
                pattern: EnvPattern::Any,
            }],
        },
    ),
    (
        AgentHarnessKey::OpenCode,
        AgentHarness {
            pretty_label: "opencode",
            repo_url: Some("https://github.com/anomalyco/opencode"),
            docs_url: Some("https://opencode.ai"),
            description: Some("Open-source AI coding agent built for the terminal."),
            family: HarnessFamily::Community,
            env_vars: &[EnvVarCheck {
                name: "OPENCODE_CLIENT",
                pattern: EnvPattern::Any,
            }],
        },
    ),
    (
        AgentHarnessKey::Pi,
        AgentHarness {
            pretty_label: "Pi",
            repo_url: Some("https://github.com/earendil-works/pi"),
            docs_url: Some("https://pi.dev"),
            description: Some(
                "Minimal, self-extensible terminal coding agent with a unified multi-provider LLM API.",
            ),
            family: HarnessFamily::Community,
            env_vars: &[EnvVarCheck {
                name: "PI_CODING_AGENT",
                pattern: EnvPattern::Any,
            }],
        },
    ),
    (
        AgentHarnessKey::Replit,
        AgentHarness {
            pretty_label: "Replit",
            repo_url: None,
            docs_url: Some("https://replit.com"),
            description: Some("Cloud development environment with an AI coding agent."),
            family: HarnessFamily::Replit,
            env_vars: &[EnvVarCheck {
                name: "REPL_ID",
                pattern: EnvPattern::Any,
            }],
        },
    ),
    (
        AgentHarnessKey::Trae,
        AgentHarness {
            pretty_label: "Trae",
            repo_url: None,
            docs_url: Some("https://trae.ai"),
            description: Some("AI-powered IDE from ByteDance."),
            family: HarnessFamily::ByteDance,
            env_vars: &[EnvVarCheck {
                name: "TRAE_AI_SHELL_ID",
                pattern: EnvPattern::Any,
            }],
        },
    ),
    (
        AgentHarnessKey::Warp,
        AgentHarness {
            pretty_label: "Warp",
            repo_url: Some("https://github.com/warpdotdev/Warp"),
            docs_url: Some("https://docs.warp.dev"),
            description: Some("AI-powered terminal with an agentic Agent Mode."),
            family: HarnessFamily::Other,
            // Exact match: TERM_PROGRAM == "WarpTerminal"
            env_vars: &[EnvVarCheck {
                name: "TERM_PROGRAM",
                pattern: EnvPattern::Exact("WarpTerminal"),
            }],
        },
    ),
    (
        AgentHarnessKey::Zed,
        AgentHarness {
            pretty_label: "Zed",
            repo_url: Some("https://github.com/zed-industries/zed"),
            docs_url: Some("https://zed.dev"),
            description: Some(
                "High-performance code editor with an integrated AI agent panel and terminal.",
            ),
            family: HarnessFamily::Other,
            env_vars: &[EnvVarCheck {
                name: "ZED_TERM",
                pattern: EnvPattern::Any,
            }],
        },
    ),
    (
        // Kept before `Cursor`: child processes inside Cursor's terminal
        // inherit CURSOR_TRACE_ID, so `cursor` must be a lower-priority
        // fallback. `cursor-cli` (CURSOR_AGENT) is the more specific signal.
        AgentHarnessKey::CursorCli,
        AgentHarness {
            pretty_label: "Cursor CLI",
            repo_url: None,
            docs_url: Some("https://cursor.com/docs/cli/overview"),
            description: Some("Cursor's coding agent for the command line."),
            family: HarnessFamily::Cursor,
            env_vars: &[EnvVarCheck {
                name: "CURSOR_AGENT",
                pattern: EnvPattern::Any,
            }],
        },
    ),
    (
        AgentHarnessKey::Cursor,
        AgentHarness {
            pretty_label: "Cursor",
            repo_url: None,
            docs_url: Some("https://cursor.com"),
            description: Some("AI-powered code editor."),
            family: HarnessFamily::Cursor,
            env_vars: &[EnvVarCheck {
                name: "CURSOR_TRACE_ID",
                pattern: EnvPattern::Any,
            }],
        },
    ),
    (
        AgentHarnessKey::Devin,
        AgentHarness {
            pretty_label: "Devin",
            repo_url: None,
            docs_url: Some("https://devin.ai"),
            description: Some("Autonomous AI software engineer from Cognition."),
            family: HarnessFamily::Cognition,
            // No specific env-var; detected only via STANDARD_AGENT_ENV_VARS.
            env_vars: &[],
        },
    ),
];

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_id_round_trips_all_keys() {
        for &key in AgentHarnessKey::ALL {
            assert_eq!(AgentHarnessKey::from_id(key.id()), Some(key));
        }
    }

    #[test]
    fn from_id_returns_none_for_unknown() {
        assert_eq!(AgentHarnessKey::from_id("unknown-agent"), None);
        assert_eq!(AgentHarnessKey::from_id(""), None);
        assert_eq!(AgentHarnessKey::from_id("unknown"), None);
    }

    #[test]
    fn unknown_variant_has_working_info() {
        let info = AgentHarnessKey::Unknown.info();
        assert_eq!(info.pretty_label, "Unknown");
        assert_eq!(info.family, HarnessFamily::Other);
        assert!(info.env_vars.is_empty());
    }

    #[test]
    fn unknown_variant_id_is_stable() {
        assert_eq!(AgentHarnessKey::Unknown.id(), "unknown");
    }

    #[test]
    fn unknown_is_not_in_all() {
        assert!(!AgentHarnessKey::ALL.contains(&AgentHarnessKey::Unknown));
    }

    #[test]
    fn all_keys_have_entry_in_registry() {
        for &key in AgentHarnessKey::ALL {
            let _ = key.info();
        }
    }

    #[test]
    fn registry_length_matches_all_array() {
        assert_eq!(AGENT_HARNESSES.len(), AgentHarnessKey::ALL.len());
    }

    #[test]
    fn pretty_labels_are_nonempty() {
        for &key in AgentHarnessKey::ALL {
            assert!(
                !key.info().pretty_label.is_empty(),
                "{key} has empty pretty_label"
            );
        }
    }

    #[test]
    fn every_harness_has_a_family() {
        for &key in AgentHarnessKey::ALL {
            let _ = key.info().family;
        }
    }

    #[test]
    fn cowork_precedes_claude_code_in_registry() {
        let pos = |target: AgentHarnessKey| {
            AGENT_HARNESSES
                .iter()
                .position(|(k, _)| *k == target)
                .unwrap()
        };
        assert!(
            pos(AgentHarnessKey::Cowork) < pos(AgentHarnessKey::ClaudeCode),
            "Cowork must appear before ClaudeCode in AGENT_HARNESSES",
        );
    }

    #[test]
    fn cursor_cli_precedes_cursor_in_registry() {
        let pos = |target: AgentHarnessKey| {
            AGENT_HARNESSES
                .iter()
                .position(|(k, _)| *k == target)
                .unwrap()
        };
        assert!(
            pos(AgentHarnessKey::CursorCli) < pos(AgentHarnessKey::Cursor),
            "CursorCli must appear before Cursor in AGENT_HARNESSES",
        );
    }

    #[test]
    fn warp_uses_exact_match() {
        let info = AgentHarnessKey::Warp.info();
        assert_eq!(info.env_vars.len(), 1);
        assert_eq!(info.env_vars[0].pattern, EnvPattern::Exact("WarpTerminal"));
    }

    #[test]
    fn devin_has_no_env_vars() {
        assert!(AgentHarnessKey::Devin.info().env_vars.is_empty());
    }

    #[test]
    fn claude_code_has_two_env_vars() {
        let info = AgentHarnessKey::ClaudeCode.info();
        assert_eq!(info.env_vars.len(), 2);
    }

    #[test]
    fn key_id_is_const_evaluable() {
        const CLAUDE_ID: &str = AgentHarnessKey::ClaudeCode.id();
        assert_eq!(CLAUDE_ID, "claude-code");
    }

    #[test]
    fn from_id_is_const_evaluable() {
        const KEY: Option<AgentHarnessKey> = AgentHarnessKey::from_id("cowork");
        assert_eq!(KEY, Some(AgentHarnessKey::Cowork));
    }

    #[test]
    fn family_ids_are_stable() {
        assert_eq!(HarnessFamily::Anthropic.id(), "anthropic");
        assert_eq!(HarnessFamily::OpenAI.id(), "openai");
        assert_eq!(HarnessFamily::Google.id(), "google");
        assert_eq!(HarnessFamily::GitHub.id(), "github");
        assert_eq!(HarnessFamily::Cognition.id(), "cognition");
    }

    #[test]
    fn anthropic_family_contains_claude_code_and_cowork() {
        assert_eq!(
            AgentHarnessKey::ClaudeCode.info().family,
            HarnessFamily::Anthropic
        );
        assert_eq!(
            AgentHarnessKey::Cowork.info().family,
            HarnessFamily::Anthropic
        );
    }

    #[test]
    fn openai_family_contains_codex() {
        assert_eq!(AgentHarnessKey::Codex.info().family, HarnessFamily::OpenAI);
    }
}
