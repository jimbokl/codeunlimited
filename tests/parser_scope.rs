use std::path::{Path, PathBuf};

fn fixture_home() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex_scope")
}

#[test]
fn codex_scope_does_not_merge_same_named_directories() {
    std::env::set_var("CODEX_HOME", fixture_home());

    let rows = codeunlimited::parsers::iter_codex(Some(Path::new("/work/client/app")));

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].unc_in, 100);
}

#[test]
fn unrelated_records_cannot_set_model_or_cwd() {
    std::env::set_var("CODEX_HOME", fixture_home());

    let rows = codeunlimited::parsers::iter_codex(Some(Path::new("/work/client/app")));

    assert_eq!(rows.len(), 1);
    assert_eq!((&*rows[0].project, &*rows[0].model), ("app", "gpt-real"));
}

#[test]
fn verbatim_unc_scope_matches_regular_unc_metadata() {
    std::env::set_var("CODEX_HOME", fixture_home());

    let rows = codeunlimited::parsers::iter_codex(Some(Path::new(r"\\?\UNC\server\share\app")));

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].unc_in, 321);
    assert_eq!(&*rows[0].model, "gpt-unc");
}
