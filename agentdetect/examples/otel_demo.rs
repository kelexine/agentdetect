//! OpenTelemetry integration demo.
//!
//! Shows what agentdetect's OTel helpers produce for env-var detections,
//! without requiring a full OTel SDK setup.  In production you'd configure
//! the SDK once at startup (`opentelemetry-otlp` exporter, Jaeger, Tempo, …)
//! and then the same calls would flow through to your real backend.
//!
//! Run:
//!     cargo run --example otel_demo --features otel
//!     CLAUDE_CODE=1 cargo run --example otel_demo --features otel

use std::time::Duration;

use agentdetect::otel;

fn main() {
    println!(
        "agentdetect v{} — OpenTelemetry integration demo\n",
        agentdetect::VERSION
    );

    println!("is_agent() = {}\n", agentdetect::is_agent());

    let detection = agentdetect::detect_from_env();
    let http_status: u16 = 200;
    let duration = Duration::from_millis(42);

    match &detection {
        Some(d) => {
            println!("─── detected: {} ───", d.label());

            // 1) Span attributes — what your tracing backend will see.
            println!("span attributes:");
            for kv in otel::attributes_for(d) {
                println!("  {} = {:?}", kv.key.as_str(), kv.value);
            }

            // 2) Metric labels — what your Prometheus / OTLP collector
            //    will use to slice the time series.
            println!("\nmetric labels:");
            for kv in otel::metric_labels_for(d) {
                println!("  {} = {:?}", kv.key.as_str(), kv.value);
            }

            // 3) Span enrichment — attaches the attributes above to the
            //    current active span (no-op when there's no span).
            otel::enrich_span(d);

            // 4) Metric emission — bumps:
            //    agentdetect.requests.total{agent_id, agent_family, status_class}
            //    agentdetect.request.duration{agent_id, agent_family}
            //    agentdetect.detections.total{agent_id, confidence, source_kind}
            otel::record_request(d, http_status, duration);
            otel::record_detection(d);

            println!("\nemitted:");
            println!(
                "  → agentdetect.requests.total {{agent_id=\"{}\", status_class=\"{}\"}} = 1",
                d.agent.id,
                otel::status_class(http_status),
            );
            println!(
                "  → agentdetect.request.duration {{agent_id=\"{}\"}} = {}s",
                d.agent.id,
                duration.as_secs_f64(),
            );
            println!(
                "  → agentdetect.detections.total {{agent_id=\"{}\", confidence=\"{}\", source_kind=\"{}\"}} = 1",
                d.agent.id,
                d.confidence.id(),
                d.source_kind().id(),
            );
        }
        None => {
            println!("no agent detected — run with CLAUDE_CODE=1 to see OTel emission");
        }
    }

    println!("\n─── what your OTel backend lets you query ───\n");
    println!("  • requests per minute by agent_id:");
    println!("      rate(agentdetect_requests_total[5m])  grouped by agent_id");
    println!();
    println!("  • success rate per agent:");
    println!("      sum(agentdetect_requests_total{{status_class=\"2xx\"}}) by (agent_id)");
    println!("        / sum(agentdetect_requests_total) by (agent_id)");
    println!();
    println!("  • % of traffic per agent:");
    println!(
        "      sum(agentdetect_requests_total) by (agent_id) / sum(agentdetect_requests_total)"
    );
    println!();
    println!("  • top-N agents by traffic:");
    println!("      topk(10, sum(agentdetect_requests_total) by (agent_id))");
    println!();
    println!("  • p99 latency per agent:");
    println!(
        "      histogram_quantile(0.99, sum(rate(agentdetect_request_duration_bucket[5m])) by (le, agent_id))"
    );
    println!();
    println!("In a real deployment, configure the OTel SDK once at startup");
    println!("(`opentelemetry-otlp` exporter, Jaeger, Tempo, Honeycomb, …) and the");
    println!("same calls above will flow through to your real backend.");
}

// Helper trait used in the demo above — print a Detection's headline.
trait DetectionLabel {
    fn label(&self) -> String;
}

impl DetectionLabel for agentdetect::Detection {
    fn label(&self) -> String {
        format!(
            "{} ({}) @ {} via {}",
            self.agent.id,
            self.agent.pretty_label,
            self.confidence.id(),
            self.source_kind().id(),
        )
    }
}
