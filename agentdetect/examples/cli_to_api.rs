//! CLI → API round-trip demo.
//!
//! Shows the full propagation flow:
//! 1. CLI detects the agent via env vars.
//! 2. CLI injects the detection onto an outgoing request via the
//!    `x-agentdetect-*` headers (using `agentdetect::propagation::inject`).
//! 3. API middleware reads those headers and reconstructs a `Detection`
//!    (using `agentdetect::propagation::read`).
//! 4. API emits OTel signals based on the reconstructed detection.
//!
//! Run:
//!     cargo run --example cli_to_api --features "http otel"
//!     CLAUDE_CODE=1 cargo run --example cli_to_api --features "http otel"

use std::time::Duration;

use agentdetect::otel;
use agentdetect::propagation;

fn main() {
    println!(
        "agentdetect v{} — CLI → API propagation round-trip demo\n",
        agentdetect::VERSION
    );

    // ── CLI side: detect from env vars ──────────────────────────────────
    println!("─── CLI side ───");
    let cli_detection = agentdetect::detect_from_env();
    match &cli_detection {
        Some(d) => {
            println!("  detected: {}", d.summary());
            println!("  → agent.id         = {}", d.agent.id);
            println!("  → confidence       = {}", d.confidence.id());
            println!("  → source.kind      = {}", d.source_kind().id());
        }
        None => println!("  no agent detected (running as human)"),
    }

    // ── CLI side: inject detection onto outgoing request ────────────────
    let mut headers = http::HeaderMap::new();
    if let Some(ref d) = cli_detection {
        propagation::inject(d, &mut headers);
        println!("\n  injected headers:");
        for (name, value) in propagation::header_pairs(d) {
            println!("    {name}: {value}");
        }
    }

    // ── API side: read detection from incoming headers ──────────────────
    println!("\n─── API side ───");
    let api_detection = propagation::read(&headers);
    match &api_detection {
        Some(d) => {
            println!("  reconstructed: {}", d.summary());
            println!("  → agent.id         = {}", d.agent.id);
            println!("  → confidence       = {}", d.confidence.id());
            println!("  → source.kind      = {}", d.source_kind().id());
            println!("    (note: source.kind is now 'propagated', not 'env-var')");

            // ── API side: emit OTel based on the reconstructed detection ─
            println!("\n  emitting OTel signals:");
            otel::enrich_span(d);
            otel::record_detection(d);
            otel::record_request(d, 200, Duration::from_millis(42));
            println!("    → span attrs: agent.id, agent.label, agentdetect.confidence, …");
            println!(
                "    → metric: agentdetect.detections.total{{agent_id=\"{}\", confidence=\"{}\", source_kind=\"propagated\"}} +1",
                d.agent.id,
                d.confidence.id()
            );
            println!(
                "    → metric: agentdetect.requests.total{{agent_id=\"{}\", status_class=\"2xx\"}} +1",
                d.agent.id
            );
        }
        None => println!("  no propagation header present — undetected request"),
    }

    println!("\n─── summary ───");
    if cli_detection.is_some() && api_detection.is_some() {
        println!("  Full round-trip succeeded: env-var detection → header → API → OTel.");
        println!("  The API knows which agent called, even though the agent never");
        println!("  touched the HTTP layer directly — only the CLI's env vars did.");
    } else {
        println!("  No agent detected.  Try:");
        println!("    CLAUDE_CODE=1 cargo run --example cli_to_api --features \"http otel\"");
    }
}
