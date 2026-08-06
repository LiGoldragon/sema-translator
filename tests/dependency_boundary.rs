use std::path::PathBuf;
use std::process::Command;

fn cargo_tree(arguments: &[&str]) -> String {
    let output = Command::new(env!("CARGO"))
        .arg("tree")
        .args(arguments)
        .current_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")))
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
fn bootstrap_feature_owns_authority_without_runtime_engine() {
    let tree = cargo_tree(&[
        "--edges",
        "normal",
        "--no-default-features",
        "--features",
        "bootstrap",
    ]);

    assert!(
        tree.contains("core-ethos "),
        "bootstrap authority needs core-ethos"
    );
    assert!(
        !tree.contains("sema-engine "),
        "bootstrap authority must not pull the runtime engine:\n{tree}"
    );
}

#[test]
fn default_features_preserve_the_complete_existing_product() {
    let tree = cargo_tree(&["--edges", "normal"]);

    assert!(
        tree.contains("core-ethos "),
        "default retains bootstrap authority"
    );
    assert!(
        tree.contains("sema-engine "),
        "default retains the runtime engine"
    );
}
