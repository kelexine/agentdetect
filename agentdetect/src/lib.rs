//! # agentdetect
//!
//! A pure Rust library that answers one question: **is this process running
//! under an AI agent harness, and if so, which one?**
//!
//! When an agent harness (Claude Code, Cursor, Codex, etc.) spawns a shell
//! to execute a command, it sets an environment variable identifying itself
//! — `CLAUDE_CODE=1`, `CURSOR_TRACE_ID=…`, `CODEX_SANDBOX=1`, etc.
//! agentdetect reads those variables and tells you which harness is active.
//!
//! ## The core use case: bit-flip output switching
//!
//! The primary purpose of agentdetect is the **bit-flip pattern**:
//!
//! ```text
//!  ┌─────────────────────────────┐
//!  │  Is an agent harness active? │
//!  └──────────────┬──────────────┘
//!           ┌─────┴─────┐
//!          NO           YES
//!           │             │
//!           ▼             ▼
//!    ┌────────────┐  ┌────────────────────┐
//!    │  Human in  │  │  Agent harness     │
//!    │  terminal  │  │  detected          │
//!    │            │  │                    │
//!    │  → Pretty  │  │  → Machine-readable│
//!    │    output  │  │    output (TSV)    │
//!    └────────────┘  └────────────────────┘
//! ```
//!
//! This is exactly how [loc-rs](https://github.com/kelexine/loc-rs) uses it:
//! when a human runs `loc`, they get a coloured summary table; when Claude
//! Code runs `loc`, it gets TSV with `# Agent-Detected: claude-code` so the
//! agent can parse the output efficiently.
//!
//! ```no_run
//! if agentdetect::is_agent() {
//!     // Agent harness is active — emit machine-readable output.
//!     let d = agentdetect::detect().unwrap();
//!     println!("# Agent-Detected: {}", d.agent.id);
//!     println!("metric\tvalue");
//!     // ... TSV data ...
//! } else {
//!     // Human terminal — emit pretty output.
//!     println!("╭──────────────────╮");
//!     println!("│  Analysis Summary │");
//!     // ... coloured table ...
//! }
//! ```
//!
//! ## Env vars are the only detection surface
//!
//! agentdetect reads **only** the process environment.  A harness spawning
//! a shell sets an env var on the process tree it spawns — that is the
//! *entire* signal.  HTTP headers and JSON bodies are NOT sniffed: a tool
//! like `curl` or `gh` running under an agent has no idea it's under an
//! agent, so its wire traffic carries no agent-identifying signal.
//!
//! ## Optional: propagation (CLI → API)
//!
//! When you're building BOTH a CLI AND an API it talks to, and you want the
//! API to know which agent is calling, enable the `http` feature and use
//! the [`propagation`] module.  The CLI detects the agent via env vars, then
//! writes the identity onto its outgoing request via a header **we define**
//! (`x-agentdetect-agent`).  The API's middleware reads that same header and
//! reconstructs a [`Detection`].  This is NOT third-party `User-Agent`
//! sniffing — the header is ours, written only by agentdetect-using code.
//!
//! ## Optional: OpenTelemetry (feature-gated)
//!
//! The `otel` feature adds span enrichment and metric emission for the
//! **secondary use case**: you're building a CLI that speaks to a public API,
//! and you want to track which agents are calling, how often, success rate,
//! % of traffic, top-N.  This is a feature on top of the core detection —
//! the core library has zero non-std dependencies.
//!
//! ## Why no behavioral detection?
//!
//! agentdetect intentionally does NOT classify "this looks like an agent"
//! based on behavioral patterns (request rate, payload shape, model string,
//! etc.).  Every detection is grounded in an explicit signal set by the
//! harness itself (env var).  This makes detection explainable and
//! impossible to silently drift — if you can't see the signal, you don't
//! classify the request.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![warn(missing_docs)]
#![warn(clippy::all)]

// ─── Public modules ───────────────────────────────────────────────────────────

pub mod detect;
pub mod detection;
pub mod pattern;
pub mod registry;

#[cfg(feature = "otel")]
#[cfg_attr(docsrs, doc(cfg(feature = "otel")))]
pub mod otel;

#[cfg(feature = "http")]
#[cfg_attr(docsrs, doc(cfg(feature = "http")))]
pub mod propagation;

// ─── Re-exports (flat public API) ─────────────────────────────────────────────

pub use detect::{
    EnvSource, ProcessEnv, detect, detect_from_env, detect_from_env_with, is_agent, is_agent_with,
};
pub use detection::{AgentInfo, Confidence, Detection, RawSignal, SourceKind};
pub use pattern::EnvPattern;
pub use registry::{AgentHarness, AgentHarnessKey, EnvVarCheck, HarnessFamily};

// ─── Crate metadata ───────────────────────────────────────────────────────────

/// Crate version (matches `CARGO_PKG_VERSION`).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Total number of harnesses the registry knows about.
///
/// Computed at compile time so dashboards can compare against this to spot
/// drift between deployed detector versions.
pub const REGISTRY_SIZE: usize = registry::AGENT_HARNESSES.len();
