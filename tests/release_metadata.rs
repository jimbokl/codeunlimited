use std::path::Path;

#[test]
fn package_declares_v1_6_msrv_and_license() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).expect("Cargo.toml");
    assert!(manifest.contains("version = \"1.6.0\""));
    assert!(manifest.contains("rust-version = \"1.82\""));

    let license = std::fs::read_to_string(root.join("LICENSE")).expect("LICENSE");
    assert!(license.contains("MIT License"));
    assert!(license.contains("Permission is hereby granted"));
}

#[test]
fn python_reference_has_a_distinct_distribution_and_command() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let pyproject = std::fs::read_to_string(root.join("pyproject.toml")).expect("pyproject.toml");
    assert!(pyproject.contains("name = \"codeunlimited-reference\""));
    assert!(pyproject.contains("codeunlimited-reference = \"codeunlimited.cli:main\""));
    assert!(!pyproject.contains("\ncodeunlimited = \"codeunlimited.cli:main\""));
}
