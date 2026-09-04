use std::path::Path;

#[test]
fn package_declares_v2_0_msrv_and_license() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).expect("Cargo.toml");
    assert!(manifest.contains("version = \"2.0.0\""));
    assert!(manifest.contains("rust-version = \"1.82\""));

    let license = std::fs::read_to_string(root.join("LICENSE")).expect("LICENSE");
    assert!(license.contains("MIT License"));
    assert!(license.contains("Permission is hereby granted"));
}

#[test]
fn release_documents_the_stateful_runtime_without_claiming_realized_savings() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let readme = std::fs::read_to_string(root.join("README.md")).expect("README.md");
    let security = std::fs::read_to_string(root.join("SECURITY.md")).expect("SECURITY.md");
    let runtime = std::fs::read_to_string(root.join("docs/RUNTIME.md")).expect("docs/RUNTIME.md");
    let combined = format!("{readme}\n{security}\n{runtime}").to_ascii_lowercase();

    assert!(readme.contains("docs/RUNTIME.md"));
    assert!(combined.contains("observation plane"));
    assert!(combined.contains("execution plane"));
    assert!(combined.contains("provider process"));
    assert!(combined.contains("does not prove realized token savings"));
    assert!(runtime.contains(".codeunlimited/runs/"));
    assert!(runtime.contains("cache_read_input_tokens"));
    assert!(runtime.contains("cache_write_input_tokens"));
    assert!(!combined.contains("guaranteed 5x"));
    assert!(!combined.contains("guaranteed 5×"));
    assert!(!combined.contains("every tool action has bounded context"));
}

#[test]
fn python_reference_has_a_distinct_distribution_and_command() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let pyproject_path = root.join("pyproject.toml");
    if !pyproject_path.exists() {
        assert!(
            !root.join("codeunlimited").exists(),
            "the excluded Python package and its metadata must stay together"
        );
        return;
    }

    let pyproject = std::fs::read_to_string(pyproject_path).expect("pyproject.toml");
    assert!(pyproject.contains("name = \"codeunlimited-reference\""));
    assert!(pyproject.contains("codeunlimited-reference = \"codeunlimited.cli:main\""));
    assert!(!pyproject.contains("\ncodeunlimited = \"codeunlimited.cli:main\""));
}
