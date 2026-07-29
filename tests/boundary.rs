use sema_translator::{
    DAEMON_BINARY_NAME, DATABASE_FILE_NAME, RUNTIME_DIRECTORY_NAME, SERVICE_NAME, SOCKET_FILE_NAME,
};

#[test]
fn dependency_and_runtime_boundaries_are_explicit() {
    let manifest = include_str!("../Cargo.toml");
    for revision in [
        "50cb4bb53ae2dc4f2516f6912be328ef98ae49f8",
        "94d2f7ee7b81d0bf3b6f1a111f6bbbc398c0e7a3",
        "0786fbe8caf27552afcdd5deb85bc82ec6088337",
        "dd8e7b5656833f640e49c099ab3be6f09881f9c5",
    ] {
        assert!(manifest.contains(revision), "missing exact pin {revision}");
    }
    assert!(!manifest.contains("signal-sema-storage"));
    assert!(!manifest.contains("\nsema-storage"));

    assert_eq!(DAEMON_BINARY_NAME, "sema-translator-daemon");
    assert_eq!(SERVICE_NAME, "sema-translator-daemon.service");
    assert_eq!(RUNTIME_DIRECTORY_NAME, "sema-translator");
    assert_eq!(SOCKET_FILE_NAME, "sema-translator.sock");
    assert_eq!(DATABASE_FILE_NAME, "sema-translator.sema");
}

#[test]
fn runtime_source_has_no_legacy_socket_or_component_document_ontology() {
    let runtime_sources = [
        include_str!("../src/store.rs"),
        include_str!("../src/runtime.rs"),
        include_str!("../src/wire.rs"),
        include_str!("../src/bin/sema-translator-daemon.rs"),
    ]
    .join("\n");
    for forbidden in [
        "/tmp",
        "--principal",
        "signal_sema_storage",
        "StoredDocument",
        "DocumentKind",
        "UniverseRegistry",
        "IdentityIntent",
        "SchemaWholeHandle",
    ] {
        assert!(
            !runtime_sources.contains(forbidden),
            "runtime contains forbidden legacy surface {forbidden}"
        );
    }
    assert!(runtime_sources.contains("peer_cred()"));
    assert!(runtime_sources.contains("--authorized-uid"));
}
