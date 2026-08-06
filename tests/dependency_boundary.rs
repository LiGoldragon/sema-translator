use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn cargo_tree(arguments: &[&str]) -> String {
    let output = Command::new(env!("CARGO"))
        .arg("tree")
        .args(arguments)
        .current_dir(repository_root())
        .output()
        .expect("cargo tree runs");
    assert!(
        output.status.success(),
        "cargo tree failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("cargo tree output is UTF-8")
}

#[test]
fn default_and_explicit_bootstrap_are_the_same_complete_surface() {
    let default = cargo_tree(&["--edges", "normal"]);
    let explicit = cargo_tree(&[
        "--edges",
        "normal",
        "--no-default-features",
        "--features",
        "bootstrap",
    ]);

    assert_eq!(default, explicit);
    for producer in ["core-ethos ", "name-table ", "content-identity "] {
        assert!(
            default.contains(producer),
            "bootstrap graph omitted {producer}"
        );
    }
    for forbidden in ["sema-engine ", "signal-sema-translator ", "tokio "] {
        assert!(
            !default.contains(forbidden),
            "bootstrap graph contains rejected runtime owner {forbidden}:\n{default}"
        );
    }
}

#[test]
fn repository_owns_no_runtime_daemon_store_or_wire_surface() {
    let root = repository_root();
    let manifest = fs::read_to_string(root.join("Cargo.toml")).expect("Cargo.toml is readable");

    for forbidden in [
        "[[bin]]",
        "runtime =",
        "sema-engine =",
        "signal-frame =",
        "tokio =",
    ] {
        assert!(
            !manifest.contains(forbidden),
            "Cargo.toml retains rejected runtime surface {forbidden}"
        );
    }
    for deleted in [
        "src/authorization.rs",
        "src/runtime.rs",
        "src/store.rs",
        "src/wire.rs",
        "src/bin/sema-translator-daemon.rs",
    ] {
        assert!(
            !Path::new(&root).join(deleted).exists(),
            "{deleted} survived"
        );
    }
}

#[test]
fn bootstrap_has_no_retired_vocabulary_producer() {
    let lock =
        fs::read_to_string(repository_root().join("Cargo.lock")).expect("Cargo.lock is readable");

    for rejected in [
        "signal-sema-translator",
        "signal-frame",
        "legacy-name-table",
    ] {
        assert!(
            !lock.contains(rejected),
            "old Frame identity {rejected} survived"
        );
    }
}

#[test]
fn public_bootstrap_surface_has_no_identity_or_receipt_constructor() {
    let source = fs::read_to_string(repository_root().join("src/bootstrap.rs"))
        .expect("bootstrap source is readable");

    for forbidden in [
        "pub struct StagedBootstrapChange",
        "pub struct BootstrapCatalog",
        "pub struct CanonicalIdentityOrder",
        "pub struct NamingAssignments",
        "pub struct GeneratedStreamAssignments",
        "pub fn mint_name",
        "pub fn mint_canonical_bytes",
        "pub fn receipt",
        "pub fn proof",
    ] {
        assert!(
            !source.contains(forbidden),
            "caller-constructible authority capability survived: {forbidden}"
        );
    }
}
