//! Entry point for running compression evaluation against real session logs.

mod eval;

use eval::discovery::SessionDiscovery;
use eval::parser::ParsedSession;
use eval::report::Report;

fn main() {
    println!("Discovering session logs...\n");

    let discoveries = SessionDiscovery::discover_all();
    println!("Found {} session files:", discoveries.len());
    for d in &discoveries {
        println!("  [{:?}] {} -> {}", d.source, d.session_id, d.path.display());
    }

    println!("\nParsing sessions...");
    let sessions: Vec<ParsedSession> = discoveries
        .iter()
        .filter_map(|d| match ParsedSession::parse(&d.path, d.source) {
            Ok(s) => {
                println!("  ✓ {} ({} turns)", d.session_id, s.turns.len());
                Some(s)
            }
            Err(e) => {
                println!("  ✗ {}: {}", d.session_id, e);
                None
            }
        })
        .filter(|s| !s.turns.is_empty())
        .collect();

    if sessions.is_empty() {
        println!("\nNo parseable sessions with turns found. Exiting.");
        return;
    }

    println!("\nEvaluating compression on {} sessions...", sessions.len());
    let summary = eval::evaluator::evaluate_sessions(&sessions);

    let report = Report::new(summary);
    println!("\n{}", report);

    // Save reports (relative to crate root via CARGO_MANIFEST_DIR)
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let report_dir = manifest_dir.join("target");
    let text_path = report_dir.join("eval-report.txt");
    let json_path = report_dir.join("eval-report.json");

    std::fs::create_dir_all(&report_dir).ok();

    match report.save_text(&text_path) {
        Ok(_) => println!("Text report saved to: {}", text_path.display()),
        Err(e) => eprintln!("Failed to save text report: {}", e),
    }

    match report.save_json(&json_path) {
        Ok(_) => println!("JSON report saved to: {}", json_path.display()),
        Err(e) => eprintln!("Failed to save JSON report: {}", e),
    }
}
