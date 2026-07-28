use std::sync::Arc;

use name_table::{EncodedId, LocalEncodedId, Name, OperationKey, TableAddress};
use sema_translator::{DispatchOutcome, Runtime, StaticAuthorizationPolicy};
use signal_sema_translator::{
    AuthorityCapability, AuthorityOperation, AuthorityReply, AuthorityRequest, AuthorityRole,
    AuthorizationClaim, CommittedReceipt, DatabaseMarker, DeclarationNode, ExpectedTableGeneration,
    NoWriteFailure, PrincipalId, ReadOperation, ReferencePath, Rename, RustVocabularyRelease,
    RustVocabularyVersion, SealCommitReceipt, SealUniversal, VocabularyEncodedId, VocabularyRoot,
    WritePrecondition,
};

const PRINCIPAL: PrincipalId = PrincipalId::new([7; 32]);

fn key(value: u8) -> OperationKey {
    OperationKey::new([value; 32])
}

fn claim(role: AuthorityRole, capability: AuthorityCapability) -> AuthorizationClaim {
    AuthorizationClaim {
        principal: PRINCIPAL,
        role,
        capability,
    }
}

fn request(operation: AuthorityOperation) -> AuthorityRequest {
    let (role, capability) = match &operation {
        AuthorityOperation::SealUniversal(_) => (
            AuthorityRole::UniversalAuthor,
            AuthorityCapability::SealUniversal,
        ),
        AuthorityOperation::Rename(_) => (
            AuthorityRole::UniversalMaintainer,
            AuthorityCapability::Rename,
        ),
        AuthorityOperation::PublishRustVocabulary(_) => (
            AuthorityRole::RustVocabularyPublisher,
            AuthorityCapability::PublishRustVocabulary,
        ),
        AuthorityOperation::Read(_) => (AuthorityRole::Reader, AuthorityCapability::Read),
    };
    AuthorityRequest {
        authorization: claim(role, capability),
        operation,
    }
}

async fn open_runtime(path: &std::path::Path) -> Runtime {
    Runtime::open(
        path,
        Arc::new(StaticAuthorizationPolicy::new().grant_all(PRINCIPAL)),
    )
    .await
    .expect("runtime opens")
}

async fn current(runtime: &Runtime) -> (DatabaseMarker, u64) {
    let outcome = runtime
        .request(
            PRINCIPAL,
            request(AuthorityOperation::Read(ReadOperation::Current)),
        )
        .await
        .expect("current read");
    match outcome.reply {
        AuthorityReply::Current(current) => {
            (current.database_marker, current.name_table_revision.value())
        }
        other => panic!("expected current authority, got {other:?}"),
    }
}

fn expected(marker: DatabaseMarker) -> WritePrecondition {
    WritePrecondition {
        database_marker: marker,
        table_generations: Vec::new(),
    }
}

async fn seal(
    runtime: &Runtime,
    operation_key: OperationKey,
    declarations: Vec<DeclarationNode>,
    references: Vec<ReferencePath>,
) -> DispatchOutcome {
    let (marker, _) = current(runtime).await;
    runtime
        .request(
            PRINCIPAL,
            request(AuthorityOperation::SealUniversal(SealUniversal {
                operation_key,
                expected: expected(marker),
                declarations,
                references,
            })),
        )
        .await
        .expect("seal request")
}

fn seal_receipt(outcome: &DispatchOutcome) -> &SealCommitReceipt {
    match &outcome.reply {
        AuthorityReply::Committed(CommittedReceipt::SealUniversal(receipt)) => receipt,
        other => panic!("expected committed seal, got {other:?}"),
    }
}

fn declaration_id(receipt: &SealCommitReceipt, spelling: &str) -> VocabularyEncodedId {
    receipt
        .name_table
        .declarations()
        .iter()
        .find(|resolved| resolved.path().spelling().as_str() == spelling)
        .expect("declaration spelling")
        .encoded_id()
        .clone()
}

async fn snapshot(
    runtime: &Runtime,
    address: TableAddress<VocabularyRoot>,
) -> signal_sema_translator::ObservedSnapshot {
    let outcome = runtime
        .request(
            PRINCIPAL,
            request(AuthorityOperation::Read(ReadOperation::CurrentSnapshot {
                address,
            })),
        )
        .await
        .expect("snapshot read");
    match outcome.reply {
        AuthorityReply::Snapshot(snapshot) => snapshot,
        other => panic!("expected snapshot, got {other:?}"),
    }
}

#[tokio::test]
async fn bootstrap_provisions_only_approved_roots_and_priors() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = open_runtime(&directory.path().join("sema-translator.sema")).await;

    let universal = snapshot(&runtime, TableAddress::root(VocabularyRoot::Universal)).await;
    assert_eq!(
        universal
            .snapshot
            .entries()
            .iter()
            .map(Name::as_str)
            .collect::<Vec<_>>(),
        ["Integer", "String"]
    );
    assert_eq!(
        universal.snapshot.address().root_variant(),
        &VocabularyRoot::Universal
    );
    let rust = snapshot(&runtime, TableAddress::root(VocabularyRoot::Rust)).await;
    assert_eq!(
        rust.snapshot
            .entries()
            .iter()
            .map(Name::as_str)
            .collect::<Vec<_>>(),
        ["u64"]
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn seal_proves_homonyms_redefinition_case_and_lookup_only_refusal() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = open_runtime(&directory.path().join("authority.sema")).await;
    let before = current(&runtime).await.0;

    let outcome = seal(
        &runtime,
        key(1),
        vec![
            DeclarationNode::Module {
                spelling: Name::new("billing"),
                declarations: vec![DeclarationNode::Member(Name::new("Status"))],
            },
            DeclarationNode::Module {
                spelling: Name::new("tasks"),
                declarations: vec![
                    DeclarationNode::Member(Name::new("Status")),
                    DeclarationNode::Member(Name::new("status")),
                ],
            },
        ],
        Vec::new(),
    )
    .await;
    let receipt = seal_receipt(&outcome);
    let status_ids = receipt
        .name_table
        .declarations()
        .iter()
        .filter(|resolved| resolved.path().spelling().as_str() == "Status")
        .map(|resolved| resolved.encoded_id().clone())
        .collect::<Vec<_>>();
    assert_eq!(status_ids.len(), 2);
    assert_ne!(status_ids[0], status_ids[1]);
    assert_ne!(
        declaration_id(receipt, "status"),
        *status_ids.last().unwrap()
    );

    let marker_after_commit = current(&runtime).await.0;
    assert!(marker_after_commit > before);
    let duplicate = seal(
        &runtime,
        key(2),
        vec![
            DeclarationNode::Member(Name::new("Duplicate")),
            DeclarationNode::Member(Name::new("Duplicate")),
        ],
        Vec::new(),
    )
    .await;
    assert!(matches!(
        duplicate.reply,
        AuthorityReply::Rejected(NoWriteFailure::Redefinition { .. })
    ));
    assert_eq!(current(&runtime).await.0, marker_after_commit);

    let unresolved = seal(
        &runtime,
        key(3),
        Vec::new(),
        vec![ReferencePath {
            root: VocabularyRoot::Universal,
            modules: Vec::new(),
            spelling: Name::new("NeverAllocated"),
        }],
    )
    .await;
    assert!(matches!(
        unresolved.reply,
        AuthorityReply::Rejected(NoWriteFailure::UnresolvedReference { .. })
    ));
    assert_eq!(current(&runtime).await.0, marker_after_commit);
    let root = snapshot(&runtime, TableAddress::root(VocabularyRoot::Universal)).await;
    assert!(
        !root
            .snapshot
            .entries()
            .iter()
            .any(|name| name.as_str() == "NeverAllocated")
    );

    let no_fallback = seal(
        &runtime,
        key(4),
        Vec::new(),
        vec![ReferencePath {
            root: VocabularyRoot::Rust,
            modules: Vec::new(),
            spelling: Name::new("Integer"),
        }],
    )
    .await;
    assert!(matches!(
        no_fallback.reply,
        AuthorityReply::Rejected(NoWriteFailure::UnresolvedReference { .. })
    ));
}

#[tokio::test]
async fn allocation_and_request_digest_are_independent_of_traversal_order() {
    let first_directory = tempfile::tempdir().unwrap();
    let second_directory = tempfile::tempdir().unwrap();
    let first = open_runtime(&first_directory.path().join("first.sema")).await;
    let second = open_runtime(&second_directory.path().join("second.sema")).await;
    let declarations = vec![
        DeclarationNode::Member(Name::new("Zulu")),
        DeclarationNode::Member(Name::new("Alpha")),
        DeclarationNode::Member(Name::new("Middle")),
    ];
    let first_receipt =
        seal_receipt(&seal(&first, key(5), declarations.clone(), Vec::new()).await).clone();
    let second_receipt = seal_receipt(
        &seal(
            &second,
            key(5),
            declarations.into_iter().rev().collect(),
            Vec::new(),
        )
        .await,
    )
    .clone();

    assert_eq!(first_receipt.request_digest, second_receipt.request_digest);
    assert_eq!(
        first_receipt.name_table.declarations(),
        second_receipt.name_table.declarations()
    );
}

#[tokio::test]
async fn text_edits_break_identity_while_operational_rename_preserves_chains() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = open_runtime(&directory.path().join("renames.sema")).await;
    let initial = seal(
        &runtime,
        key(10),
        vec![DeclarationNode::Module {
            spelling: Name::new("billing"),
            declarations: vec![DeclarationNode::Member(Name::new("Status"))],
        }],
        Vec::new(),
    )
    .await;
    let initial_receipt = seal_receipt(&initial);
    let billing = declaration_id(initial_receipt, "billing");
    let status = declaration_id(initial_receipt, "Status");

    let text_edit = seal(
        &runtime,
        key(11),
        vec![DeclarationNode::Module {
            spelling: Name::new("billing"),
            declarations: vec![DeclarationNode::Member(Name::new("State"))],
        }],
        Vec::new(),
    )
    .await;
    let state = declaration_id(seal_receipt(&text_edit), "State");
    assert_ne!(status, state);

    let reappearance = seal(
        &runtime,
        key(12),
        vec![DeclarationNode::Module {
            spelling: Name::new("billing"),
            declarations: vec![DeclarationNode::Member(Name::new("Status"))],
        }],
        Vec::new(),
    )
    .await;
    assert_eq!(
        declaration_id(seal_receipt(&reappearance), "Status"),
        status
    );

    let (marker, _) = current(&runtime).await;
    let renamed = runtime
        .request(
            PRINCIPAL,
            request(AuthorityOperation::Rename(Rename {
                operation_key: key(13),
                expected: expected(marker),
                target: billing.clone(),
                new_spelling: Name::new("accounts"),
            })),
        )
        .await
        .unwrap();
    let renamed_receipt = match &renamed.reply {
        AuthorityReply::Committed(CommittedReceipt::Rename(receipt)) => receipt,
        other => panic!("expected rename, got {other:?}"),
    };
    assert_eq!(renamed_receipt.name_table.target(), &billing);
    let child = snapshot(&runtime, billing.child_table()).await;
    assert!(
        child
            .snapshot
            .entries()
            .iter()
            .any(|name| name.as_str() == "Status")
    );

    let module_text_edit = seal(
        &runtime,
        key(14),
        vec![DeclarationNode::Module {
            spelling: Name::new("workflow"),
            declarations: vec![DeclarationNode::Member(Name::new("Status"))],
        }],
        Vec::new(),
    )
    .await;
    let workflow_status = declaration_id(seal_receipt(&module_text_edit), "Status");
    assert_ne!(workflow_status, status);
    assert_ne!(workflow_status.chain().first(), status.chain().first());
}

#[tokio::test]
async fn immutable_precedence_authorization_and_stale_writes_leave_no_trace() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = open_runtime(&directory.path().join("refusals.sema")).await;
    let before = current(&runtime).await.0;

    let unknown_rust =
        EncodedId::new(VocabularyRoot::Rust, vec![LocalEncodedId::new(u16::MAX)]).unwrap();
    let immutable = runtime
        .request(
            PRINCIPAL,
            request(AuthorityOperation::Rename(Rename {
                operation_key: key(20),
                expected: expected(before),
                target: unknown_rust,
                new_spelling: Name::new("other"),
            })),
        )
        .await
        .unwrap();
    assert!(matches!(
        immutable.reply,
        AuthorityReply::Rejected(NoWriteFailure::ImmutableTable { .. })
    ));
    assert_eq!(current(&runtime).await.0, before);

    let unauthorized = runtime
        .request(
            PRINCIPAL,
            AuthorityRequest {
                authorization: AuthorizationClaim {
                    principal: PrincipalId::new([8; 32]),
                    role: AuthorityRole::UniversalAuthor,
                    capability: AuthorityCapability::SealUniversal,
                },
                operation: AuthorityOperation::SealUniversal(SealUniversal {
                    operation_key: key(21),
                    expected: expected(before),
                    declarations: vec![DeclarationNode::Member(Name::new("Denied"))],
                    references: Vec::new(),
                }),
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        unauthorized.reply,
        AuthorityReply::Rejected(NoWriteFailure::AuthorizationDenied { .. })
    ));
    assert_eq!(current(&runtime).await.0, before);

    let stale = runtime
        .request(
            PRINCIPAL,
            request(AuthorityOperation::SealUniversal(SealUniversal {
                operation_key: key(22),
                expected: WritePrecondition {
                    database_marker: before,
                    table_generations: vec![ExpectedTableGeneration {
                        address: TableAddress::root(VocabularyRoot::Universal),
                        generation: u64::MAX,
                    }],
                },
                declarations: vec![DeclarationNode::Member(Name::new("Stale"))],
                references: Vec::new(),
            })),
        )
        .await
        .unwrap();
    assert!(matches!(
        stale.reply,
        AuthorityReply::Rejected(NoWriteFailure::StaleTableGeneration { .. })
    ));
    assert_eq!(current(&runtime).await.0, before);
}

#[tokio::test]
async fn multi_table_failure_rolls_back_and_events_follow_only_commits() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = open_runtime(&directory.path().join("atomic.sema")).await;
    let mut events = runtime.subscribe();
    let before_marker = current(&runtime).await.0;
    let before_root = snapshot(&runtime, TableAddress::root(VocabularyRoot::Universal)).await;

    let failed = seal(
        &runtime,
        key(25),
        vec![
            DeclarationNode::Module {
                spelling: Name::new("rollback_a"),
                declarations: vec![DeclarationNode::Member(Name::new("One"))],
            },
            DeclarationNode::Module {
                spelling: Name::new("rollback_b"),
                declarations: vec![DeclarationNode::Member(Name::new("Two"))],
            },
        ],
        vec![ReferencePath {
            root: VocabularyRoot::Universal,
            modules: vec![Name::new("rollback_a")],
            spelling: Name::new("Missing"),
        }],
    )
    .await;
    assert!(matches!(
        failed.reply,
        AuthorityReply::Rejected(NoWriteFailure::UnresolvedReference { .. })
    ));
    assert_eq!(current(&runtime).await.0, before_marker);
    let after_root = snapshot(&runtime, TableAddress::root(VocabularyRoot::Universal)).await;
    assert_eq!(after_root.snapshot, before_root.snapshot);
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(20), events.recv())
            .await
            .is_err()
    );

    let committed = seal(
        &runtime,
        key(26),
        vec![DeclarationNode::Member(Name::new("Committed"))],
        Vec::new(),
    )
    .await;
    assert!(matches!(
        committed.reply,
        AuthorityReply::Committed(CommittedReceipt::SealUniversal(_))
    ));
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(1), events.recv())
            .await
            .expect("commit event arrives")
            .is_ok()
    );

    let stale = runtime
        .request(
            PRINCIPAL,
            request(AuthorityOperation::SealUniversal(SealUniversal {
                operation_key: key(27),
                expected: expected(before_marker),
                declarations: vec![DeclarationNode::Member(Name::new("TooLate"))],
                references: Vec::new(),
            })),
        )
        .await
        .unwrap();
    assert!(matches!(
        stale.reply,
        AuthorityReply::Rejected(NoWriteFailure::StaleDatabaseMarker { .. })
    ));
}

#[tokio::test]
async fn rust_publication_is_monotonic_append_only() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = open_runtime(&directory.path().join("rust.sema")).await;
    let (marker, _) = current(&runtime).await;
    let first = runtime
        .request(
            PRINCIPAL,
            request(AuthorityOperation::PublishRustVocabulary(
                RustVocabularyRelease {
                    operation_key: key(30),
                    version: RustVocabularyVersion::new(1),
                    expected: expected(marker),
                    declarations: vec![DeclarationNode::Member(Name::new("struct"))],
                },
            )),
        )
        .await
        .unwrap();
    assert!(matches!(
        first.reply,
        AuthorityReply::Committed(CommittedReceipt::PublishRustVocabulary(_))
    ));
    let after_first = current(&runtime).await.0;

    let conflict = runtime
        .request(
            PRINCIPAL,
            request(AuthorityOperation::PublishRustVocabulary(
                RustVocabularyRelease {
                    operation_key: key(31),
                    version: RustVocabularyVersion::new(1),
                    expected: expected(after_first),
                    declarations: vec![DeclarationNode::Member(Name::new("enum"))],
                },
            )),
        )
        .await
        .unwrap();
    assert!(matches!(
        conflict.reply,
        AuthorityReply::Rejected(NoWriteFailure::RustVocabularyVersionConflict { .. })
    ));
    assert_eq!(current(&runtime).await.0, after_first);

    let second = runtime
        .request(
            PRINCIPAL,
            request(AuthorityOperation::PublishRustVocabulary(
                RustVocabularyRelease {
                    operation_key: key(32),
                    version: RustVocabularyVersion::new(2),
                    expected: expected(after_first),
                    declarations: vec![
                        DeclarationNode::Member(Name::new("struct")),
                        DeclarationNode::Member(Name::new("pub")),
                    ],
                },
            )),
        )
        .await
        .unwrap();
    assert!(matches!(
        second.reply,
        AuthorityReply::Committed(CommittedReceipt::PublishRustVocabulary(_))
    ));
    let rust = snapshot(&runtime, TableAddress::root(VocabularyRoot::Rust)).await;
    assert_eq!(
        rust.snapshot
            .entries()
            .iter()
            .map(Name::as_str)
            .collect::<Vec<_>>(),
        ["u64", "struct", "pub"]
    );
}

#[tokio::test]
async fn restart_preserves_history_and_lost_reply_replays_original_marker() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("restart.sema");
    let runtime = open_runtime(&path).await;
    let (base, _) = current(&runtime).await;
    let original_operation = SealUniversal {
        operation_key: key(40),
        expected: expected(base),
        declarations: vec![DeclarationNode::Member(Name::new("BeforeRename"))],
        references: Vec::new(),
    };
    let committed = runtime
        .request(
            PRINCIPAL,
            request(AuthorityOperation::SealUniversal(
                original_operation.clone(),
            )),
        )
        .await
        .unwrap();
    let original = seal_receipt(&committed).clone();
    let change = original
        .name_table
        .changed_tables()
        .first()
        .expect("root changed");
    let historical_address = change.address().clone();
    let historical_digest = change.snapshot();
    let target = declaration_id(&original, "BeforeRename");
    let expected_target = target.clone();

    let replay = runtime
        .request(
            PRINCIPAL,
            request(AuthorityOperation::SealUniversal(original_operation)),
        )
        .await
        .unwrap();
    assert_eq!(replay.reply, committed.reply);
    assert!(replay.event.is_none());

    let (marker, _) = current(&runtime).await;
    let renamed = runtime
        .request(
            PRINCIPAL,
            request(AuthorityOperation::Rename(Rename {
                operation_key: key(41),
                expected: expected(marker),
                target,
                new_spelling: Name::new("AfterRename"),
            })),
        )
        .await
        .unwrap();
    match renamed.reply {
        AuthorityReply::Committed(CommittedReceipt::Rename(receipt)) => {
            assert_eq!(receipt.name_table.target(), &expected_target);
        }
        other => panic!("expected member rename receipt, got {other:?}"),
    }
    runtime.shutdown().await.unwrap();

    let reopened = open_runtime(&path).await;
    let recovered = reopened
        .request(
            PRINCIPAL,
            request(AuthorityOperation::Read(ReadOperation::CommittedReceipt {
                operation_key: key(40),
            })),
        )
        .await
        .unwrap();
    assert_eq!(
        recovered.reply,
        AuthorityReply::Receipt(CommittedReceipt::SealUniversal(original.clone()))
    );
    let historical = reopened
        .request(
            PRINCIPAL,
            request(AuthorityOperation::Read(
                ReadOperation::HistoricalSnapshot {
                    address: historical_address,
                    snapshot: historical_digest,
                },
            )),
        )
        .await
        .unwrap();
    match historical.reply {
        AuthorityReply::Snapshot(found) => assert!(
            found
                .snapshot
                .entries()
                .iter()
                .any(|name| name.as_str() == "BeforeRename")
        ),
        other => panic!("expected historical snapshot, got {other:?}"),
    }
}

#[tokio::test]
async fn complete_local_id_range_is_used_before_explicit_exhaustion() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = open_runtime(&directory.path().join("capacity.sema")).await;
    let declarations = (0..65_534)
        .map(|index| DeclarationNode::Member(Name::new(format!("Member{index:05}"))))
        .collect();
    let filled = seal(&runtime, key(50), declarations, Vec::new()).await;
    assert!(matches!(
        filled.reply,
        AuthorityReply::Committed(CommittedReceipt::SealUniversal(_))
    ));
    let full_marker = current(&runtime).await.0;
    let root = snapshot(&runtime, TableAddress::root(VocabularyRoot::Universal)).await;
    assert_eq!(root.snapshot.entries().len(), 65_536);

    let overflow = seal(
        &runtime,
        key(51),
        vec![DeclarationNode::Member(Name::new("OneTooMany"))],
        Vec::new(),
    )
    .await;
    assert!(matches!(
        overflow.reply,
        AuthorityReply::Rejected(NoWriteFailure::TableCapacity { .. })
    ));
    assert_eq!(current(&runtime).await.0, full_marker);
}
