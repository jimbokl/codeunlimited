//! Golden-fixture integration test: synthetic Claude Code and Codex logs with
//! known token values, run through the real parsers, detectors and reports.

use std::path::{Path, PathBuf};

use codeunlimited::{detectors, metrics, parsers, report, reportcmd};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

#[test]
fn golden_end_to_end() {
    // Env vars steer parsers at fixture roots. Single test fn => no data race.
    std::env::set_var("CLAUDE_HOME", fixtures().join("claude_home"));
    std::env::set_var("CODEX_HOME", fixtures().join("codex_home"));

    // --- Claude: dedupe by message id, skip <synthetic>, TTL breakdown ---
    let claude = parsers::iter_claude(None);
    assert_eq!(claude.len(), 2, "duplicate m1 deduped, synthetic skipped");
    let r1 = &claude[0];
    assert_eq!(r1.source, "claude");
    assert_eq!(r1.project, "C--test-proj");
    assert_eq!(r1.model, "claude-opus-5");
    assert_eq!(
        (r1.unc_in, r1.cached_in, r1.w5, r1.w1h, r1.out),
        (100, 0, 30000, 0, 500)
    );
    assert_eq!(r1.prompt_total(), 30100);
    let r2 = &claude[1];
    assert_eq!(
        (r2.unc_in, r2.cached_in, r2.w5, r2.out),
        (5, 30000, 1000, 200)
    );
    assert!(r1.ts.is_some() && r2.ts.unwrap() > r1.ts.unwrap());

    // --- Codex: model/cwd sticky per file, rate-limit-only events skipped ---
    let codex = parsers::iter_codex(None);
    assert_eq!(
        codex.len(),
        2,
        "rate-limit-only token_count carries no usage"
    );
    let c1 = &codex[0];
    assert_eq!(c1.source, "codex");
    assert_eq!(c1.model, "gpt-5.5");
    assert_eq!(c1.project, "testcx");
    assert_eq!((c1.unc_in, c1.cached_in, c1.out), (200, 800, 50));
    let c2 = &codex[1];
    assert_eq!((c2.unc_in, c2.cached_in, c2.out), (100, 1900, 100));

    // --- Detectors + reports run clean on the combined set ---
    let mut all = claude.clone();
    all.extend(codex.iter().cloned());
    let findings = detectors::run_all(&all);
    assert_eq!(findings.len(), 4);
    let text = report::render(&all, &findings);
    assert!(text.contains("CODEUNLIMITED"));
    assert!(text.contains("claude"));
    assert!(text.contains("codex"));
    let json = report::render_json(&all, &findings);
    let v: serde_json::Value = serde_json::from_str(&json).expect("valid json");
    assert_eq!(v["sources"]["claude"]["requests"], 2);
    assert_eq!(v["sources"]["codex"]["requests"], 2);

    // --- Markdown report builder (pure, no filesystem) ---
    let delta = reportcmd::DeltaInfo {
        since: "2026-01-01".into(),
        b_requests: 2,
        b_prompt: 40_000.0,
        b_growth: 2.0,
        now: metrics::compute(&claude),
    };
    let history = vec![serde_json::json!({
        "date": "2026-01-02", "requests": 4, "avg_prompt_per_turn": 16000,
        "context_growth": 1.0, "reclaimable_tokens": 30000u64,
    })];
    let md = reportcmd::build_markdown(
        "test-proj",
        &all,
        &findings,
        Some(&delta),
        &history,
        "2026-01-02",
    );
    assert!(md.starts_with("# codeunlimited report - test-proj"));
    assert!(md.contains("## Where the limit leaks"));
    assert!(md.contains("## Delta since baseline (2026-01-01)"));
    assert!(md.contains("| 2026-01-02 | 4 | 16k | 1.0x | 0 |"));
    assert!(md.contains("| claude | 2 |"));
    assert!(md.contains("| codex | 2 |"));

    // --- Project key sanitizer (path -> claude log dir name) ---
    assert_eq!(
        parsers::claude_project_key(Path::new("C:\\test\\proj")),
        "C--test-proj"
    );
}
