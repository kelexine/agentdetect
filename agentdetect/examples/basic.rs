//! Basic detection from the process environment.
//!
//! Run:
//!     cargo run --example basic
//!     AI_AGENT=claude-code cargo run --example basic
//!     CLAUDE_CODE=1      cargo run --example basic
//!     CRUSH=1 AI_AGENT=codex cargo run --example basic   # specific beats standard

use agentdetect::{Confidence, SourceKind};

fn main() {
    println!(
        "agentdetect v{} — {} harnesses registered\n",
        agentdetect::VERSION,
        agentdetect::REGISTRY_SIZE,
    );

    // ── The bit-flip primitive ──────────────────────────────────────────
    println!("is_agent() = {}", agentdetect::is_agent());
    println!();

    match agentdetect::detect_from_env() {
        Some(d) => {
            println!("DETECTED: {}", d.summary());
            println!();
            println!("  agent.id         = {}", d.agent.id);
            println!("  agent.label      = {}", d.agent.pretty_label);
            println!("  agent.family     = {}", d.agent.family);
            println!("  agent.version    = {:?}", d.agent.version);
            if let Some(url) = d.agent.repo_url {
                println!("  agent.repo_url   = {url}");
            }
            if let Some(url) = d.agent.docs_url {
                println!("  agent.docs_url   = {url}");
            }
            println!();
            println!("  confidence       = {}", d.confidence.id());
            println!("  source.kind      = {}", d.source_kind().id());
            println!("  source.name      = {}", d.primary_signal.signal_name());
            println!();
            println!("  evidence trail ({} signal(s)):", d.raw_signals.len());
            for s in &d.raw_signals {
                println!("    - {s:?}");
            }

            // Demonstrate the typed enums for downstream branching.
            match (d.confidence, d.source_kind()) {
                (Confidence::High, SourceKind::EnvVar) => {
                    println!(
                        "\n  → high-confidence local detection; safe to apply agent-specific behaviour"
                    );
                }
                (Confidence::High, SourceKind::Propagated) => {
                    println!(
                        "\n  → high-confidence propagated detection; trusted CLI forwarded this"
                    );
                }
                (Confidence::Medium, _) => {
                    println!("\n  → medium confidence; treat as advisory until corroborated");
                }
                (Confidence::Low, _) => {
                    println!(
                        "\n  → low confidence; new/unknown harness — log for visibility, don't act on it"
                    );
                }
            }
        }
        None => {
            println!("No agent harness detected from process environment.");
            println!();
            println!("Try one of:");
            println!("    AI_AGENT=claude-code cargo run --example basic");
            println!("    CLAUDE_CODE=1      cargo run --example basic");
            println!("    CRUSH=1            cargo run --example basic");
        }
    }
}
