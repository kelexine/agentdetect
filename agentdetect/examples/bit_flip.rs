//! The canonical agentdetect use case: bit-flip output switching.
//!
//! This is exactly the pattern loc-rs uses — when a human runs the tool,
//! they get pretty terminal output; when an agent harness runs it, they
//! get machine-readable TSV.  Run it both ways to see the difference:
//!
//!     cargo run --example bit_flip
//!     CLAUDE_CODE=1 cargo run --example bit_flip
//!     CRUSH=1 cargo run --example bit_flip
//!     AI_AGENT=codex cargo run --example bit_flip

fn main() {
    // ── Some fake "analysis" we want to present ────────────────────────
    let total_lines = 3_388;
    let total_code = 2_387;
    let total_comment = 575;
    let total_blank = 426;
    let text_files = 25;

    // ── The bit-flip: one call, two output paths ──────────────────────
    match agentdetect::detect_from_env() {
        Some(d) => {
            // ─── Agent detected → machine-readable TSV ────────────────
            //
            // The `# Agent-Detected:` header line is the convention loc-rs
            // established — agents look for it to confirm the tool knows
            // they're calling.  Everything else is tab-separated so an
            // agent can parse it with `split('\t')` without regex.
            println!("# Agent-Detected: {}", d.agent.id);
            println!("metric\tvalue");
            println!("total_lines\t{total_lines}");
            println!("total_code\t{total_code}");
            println!("total_comment\t{total_comment}");
            println!("total_blank\t{total_blank}");
            println!("text_files\t{text_files}");

            eprintln!(
                "# Detection: {} ({}) @ {} via {} {}",
                d.agent.id,
                d.agent.pretty_label,
                d.confidence.id(),
                d.source_kind().id(),
                d.primary_signal.signal_name(),
            );
        }
        None => {
            // ─── No agent → human terminal → pretty output ────────────
            //
            // Coloured, padded, human-friendly.  No `# Agent-Detected:`
            // header, no TSV — just a nice summary table.
            println!();
            println!("  LOC-RS ANALYSIS SUMMARY");
            println!(
                "  ────────────────────────────────────────────────────────────────────────────"
            );
            println!(
                "  Total Lines of Code    : {total_lines:<20} Text Files         : {text_files}",
            );
            println!("  Code / Comment / Blank : {total_code} / {total_comment} / {total_blank}");
            println!(
                "  ────────────────────────────────────────────────────────────────────────────"
            );
            println!();
        }
    }
}
