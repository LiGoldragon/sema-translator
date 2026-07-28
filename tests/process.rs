use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::os::unix::net::UnixStream as StdUnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use name_table::{Name, OperationKey};
use rkyv::{Archive, Deserialize, Serialize};
use sema_engine::{
    Assertion, Engine, EngineOpen, EngineRecord, FamilyName, RecordKey, Retraction, SchemaHash,
    SchemaVersion, TableDescriptor, TableName, TableReference, VersionedStoreName,
    VersioningPolicy,
};
use sema_translator::{
    AUTHORITY_ROUTE, Runtime, StaticAuthorizationPolicy, principal_for_unix_uid,
};
use signal_frame::{
    ExchangeIdentifier, ExchangeLane, LaneSequence, Reply, Request, RootCode, SessionEpoch,
    StreamingFrameBody, SubReply, VariantCode, WireRoute,
};
use signal_sema_translator::{
    AuthorityCapability, AuthorityOperation, AuthorityReply, AuthorityRequest,
    AuthorityRequestDigest, AuthorityRole, AuthorizationClaim, CommittedReceipt, DatabaseMarker,
    DeclarationNode, NoWriteFailure, PrincipalId, ReadOperation, Rename, RootRecord,
    RustVocabularyVersion, SealUniversal, TranslatorFrame, VocabularyRoot, WritePrecondition,
};

#[derive(Archive, Serialize, Deserialize, Clone, Debug)]
struct StoredRustRelease {
    version: RustVocabularyVersion,
    digest: AuthorityRequestDigest,
}

#[derive(Archive, Serialize, Deserialize, Clone, Debug)]
struct StoredAuthorityState {
    magic: [u8; 8],
    archive_version: u16,
    roots: Vec<Vec<u8>>,
    name_table_archive: Vec<u8>,
    current_rust_version: RustVocabularyVersion,
    rust_releases: Vec<StoredRustRelease>,
}

#[derive(Archive, Serialize, Deserialize, Clone, Debug)]
enum StoredAuthorityRecord {
    State(StoredAuthorityState),
    Receipt(CommittedReceipt),
}

impl EngineRecord for StoredAuthorityRecord {
    fn record_key(&self) -> RecordKey {
        match self {
            Self::State(_) => RecordKey::new("authority-state"),
            Self::Receipt(receipt) => receipt_record_key(receipt.operation_key()),
        }
    }
}

fn receipt_record_key(operation_key: OperationKey) -> RecordKey {
    let mut key = String::from("receipt:");
    for byte in operation_key.bytes() {
        use std::fmt::Write as _;
        write!(&mut key, "{byte:02x}").unwrap();
    }
    RecordKey::new(key)
}

fn process_uid() -> u32 {
    std::fs::metadata(".")
        .expect("current directory metadata")
        .uid()
}

fn process_principal() -> PrincipalId {
    principal_for_unix_uid(process_uid())
}

struct Daemon {
    child: Child,
    socket: PathBuf,
}

impl Daemon {
    fn start(socket: &Path, database: &Path) -> Self {
        let mut child = spawn_daemon(socket, database);
        let stdout = child.stdout.take().expect("captured daemon stdout");
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let bytes = reader.read_line(&mut line).expect("read READY line");
        assert!(bytes > 0, "daemon exited before READY");
        assert!(
            line.starts_with("READY "),
            "unexpected daemon readiness line: {line:?}"
        );
        Self {
            child,
            socket: socket.to_path_buf(),
        }
    }

    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let deadline = Instant::now() + Duration::from_secs(2);
        while self.socket.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        if self.socket.exists() {
            std::fs::remove_file(&self.socket).expect("remove stale test socket");
        }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        self.stop();
    }
}

fn spawn_daemon(socket: &Path, database: &Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_sema-translator-daemon"))
        .arg("daemon")
        .arg("--socket")
        .arg(socket)
        .arg("--database")
        .arg(database)
        .arg("--authorized-uid")
        .arg(process_uid().to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn daemon")
}

fn key(value: u8) -> OperationKey {
    OperationKey::new([value; 32])
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
        authorization: AuthorizationClaim {
            principal: process_principal(),
            role,
            capability,
        },
        operation,
    }
}

fn current(socket: &Path) -> DatabaseMarker {
    match exchange(
        socket,
        request(AuthorityOperation::Read(ReadOperation::Current)),
        false,
    )
    .0
    {
        AuthorityReply::Current(current) => current.database_marker,
        other => panic!("expected current marker, got {other:?}"),
    }
}

fn exchange(
    socket: &Path,
    request: AuthorityRequest,
    expect_event: bool,
) -> (
    AuthorityReply,
    Option<signal_sema_translator::PostCommitEvent>,
) {
    let exchange = ExchangeIdentifier::new(
        SessionEpoch::new(1),
        ExchangeLane::Connector,
        LaneSequence::first(),
    );
    let frame = TranslatorFrame::new(
        AUTHORITY_ROUTE,
        StreamingFrameBody::Request {
            exchange,
            request: Request::from_payload(request),
        },
    );
    let mut stream = StdUnixStream::connect(socket).expect("connect daemon");
    stream
        .write_all(&frame.encode_length_prefixed().expect("encode request"))
        .expect("write request");
    let reply =
        TranslatorFrame::decode_length_prefixed(&read_frame(&mut stream)).expect("decode reply");
    let reply = match reply.into_body() {
        StreamingFrameBody::Reply {
            reply: Reply::Accepted { per_operation, .. },
            ..
        } => match per_operation.head() {
            SubReply::Ok(reply) => reply.clone(),
            SubReply::Failed {
                detail: Some(reply),
                ..
            } => reply.clone(),
            other => panic!("unexpected subreply: {other:?}"),
        },
        other => panic!("expected reply frame, got {other:?}"),
    };
    let event = if expect_event {
        match TranslatorFrame::decode_length_prefixed(&read_frame(&mut stream))
            .expect("decode commit event")
            .into_body()
        {
            StreamingFrameBody::SubscriptionEvent { event, .. } => Some(event),
            other => panic!("expected event frame, got {other:?}"),
        }
    } else {
        None
    };
    (reply, event)
}

fn read_frame(stream: &mut StdUnixStream) -> Vec<u8> {
    let mut length = [0; 4];
    stream.read_exact(&mut length).expect("read frame length");
    let length = u32::from_be_bytes(length) as usize;
    let mut bytes = Vec::with_capacity(length + 4);
    bytes.extend_from_slice(&(length as u32).to_be_bytes());
    bytes.resize(length + 4, 0);
    stream.read_exact(&mut bytes[4..]).expect("read frame body");
    bytes
}

fn open_fault_engine(database: &Path) -> (Engine, TableReference<StoredAuthorityRecord>) {
    let mut engine = Engine::open(
        EngineOpen::new(database, SchemaVersion::new(1)).with_versioning(VersioningPolicy::new(
            VersionedStoreName::new("sema-translator"),
        )),
    )
    .expect("open isolated database for typed fault injection");
    let table = engine
        .register_table(TableDescriptor::<StoredAuthorityRecord>::new(
            TableName::new("sema_translator_authority"),
            FamilyName::new("sema-translator-authority"),
            SchemaHash::for_label("sema-translator-authority-v1"),
        ))
        .expect("register matching test family");
    (engine, table)
}

fn assert_refuses_readiness(socket: &Path, database: &Path) {
    let mut child = spawn_daemon(socket, database);
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll invalid daemon") {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("invalid daemon did not fail readiness");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(!status.success());
    assert!(!socket.exists());
}

#[test]
fn daemon_restarts_replays_lost_reply_and_socket_absence_fails_closed() {
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("sema-translator.sock");
    let database = directory.path().join("sema-translator.sema");
    assert!(StdUnixStream::connect(&socket).is_err());

    let mut daemon = Daemon::start(&socket, &database);
    let base = current(&socket);
    let operation = SealUniversal {
        operation_key: key(1),
        expected: WritePrecondition {
            database_marker: base,
            table_generations: Vec::new(),
        },
        declarations: vec![DeclarationNode::Member(Name::new("ProcessWitness"))],
        references: Vec::new(),
    };
    let (committed, event) = exchange(
        &socket,
        request(AuthorityOperation::SealUniversal(operation.clone())),
        true,
    );
    let receipt = match committed {
        AuthorityReply::Committed(receipt @ CommittedReceipt::SealUniversal(_)) => receipt,
        other => panic!("expected process seal, got {other:?}"),
    };
    assert!(event.is_some());

    daemon.stop();
    assert!(StdUnixStream::connect(&socket).is_err());
    let mut reopened = Daemon::start(&socket, &database);
    let recovered = exchange(
        &socket,
        request(AuthorityOperation::Read(ReadOperation::CommittedReceipt {
            operation_key: key(1),
        })),
        false,
    )
    .0;
    assert_eq!(recovered, AuthorityReply::Receipt(receipt.clone()));

    let replay = exchange(
        &socket,
        request(AuthorityOperation::SealUniversal(operation)),
        false,
    )
    .0;
    assert_eq!(replay, AuthorityReply::Committed(receipt));
    reopened.stop();
    assert!(StdUnixStream::connect(&socket).is_err());
}

#[test]
fn wrong_contract_and_revision_are_refused_before_malformed_body_decode() {
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("bound.sock");
    let database = directory.path().join("bound.sema");
    let mut daemon = Daemon::start(&socket, &database);
    let before = current(&socket);

    let exchange_id = ExchangeIdentifier::new(
        SessionEpoch::new(2),
        ExchangeLane::Connector,
        LaneSequence::first(),
    );
    let frame = TranslatorFrame::new(
        AUTHORITY_ROUTE,
        StreamingFrameBody::Request {
            exchange: exchange_id,
            request: Request::from_payload(request(AuthorityOperation::Read(
                ReadOperation::Current,
            ))),
        },
    );
    let original = frame.encode_length_prefixed().unwrap();
    for (header_index, replacement) in [(4usize, 99u8), (8usize, 2u8)] {
        let mut malformed = original.clone();
        malformed[header_index] = replacement;
        malformed.truncate(4 + signal_frame::SHORT_HEADER_BYTE_COUNT + 1);
        let length = (malformed.len() - 4) as u32;
        malformed[..4].copy_from_slice(&length.to_be_bytes());
        let mut stream = StdUnixStream::connect(&socket).unwrap();
        stream.write_all(&malformed).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut byte = [0];
        assert_eq!(
            stream
                .read(&mut byte)
                .expect("connection closes on bad binding"),
            0
        );
    }

    let wrong_route = TranslatorFrame::new(
        WireRoute::new(RootCode::new(9), VariantCode::new(9)),
        StreamingFrameBody::Request {
            exchange: exchange_id,
            request: Request::from_payload(request(AuthorityOperation::Read(
                ReadOperation::Current,
            ))),
        },
    );
    let mut stream = StdUnixStream::connect(&socket).unwrap();
    stream
        .write_all(&wrong_route.encode_length_prefixed().unwrap())
        .unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut byte = [0];
    assert_eq!(stream.read(&mut byte).expect("unknown route closes"), 0);

    let mut spoofed = request(AuthorityOperation::Read(ReadOperation::Current));
    spoofed.authorization.principal = PrincipalId::new([7; 32]);
    match exchange(&socket, spoofed, false).0 {
        AuthorityReply::Rejected(NoWriteFailure::AuthorizationDenied { principal, .. }) => {
            assert_eq!(principal, process_principal())
        }
        other => panic!("expected kernel-authenticated refusal, got {other:?}"),
    }
    assert_eq!(current(&socket), before);
    daemon.stop();
}

#[test]
fn a_second_process_cannot_replace_a_live_listener() {
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("live.sock");
    let database = directory.path().join("live.sema");
    let mut daemon = Daemon::start(&socket, &database);
    let before = current(&socket);

    let mut second = spawn_daemon(&socket, &database);
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = second.try_wait().expect("poll second daemon") {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = second.kill();
            let _ = second.wait();
            panic!("second daemon did not refuse the live socket");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(!status.success());
    assert!(socket.exists());
    assert_eq!(current(&socket), before);
    daemon.stop();
}

#[test]
fn socket_binding_refuses_an_untrusted_runtime_directory() {
    let directory = tempfile::tempdir().unwrap();
    let insecure = directory.path().join("insecure");
    std::fs::create_dir(&insecure).unwrap();
    std::fs::set_permissions(&insecure, std::fs::Permissions::from_mode(0o777)).unwrap();
    let socket = insecure.join("authority.sock");
    assert!(sema_translator::wire::bind(&socket).is_err());
    assert!(!socket.exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn stopped_authority_actor_returns_a_typed_unavailable_reply() {
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("unavailable.sock");
    let database = directory.path().join("unavailable.sema");
    let principal = process_principal();
    let runtime = Runtime::open(
        &database,
        Arc::new(StaticAuthorizationPolicy::new().grant_all(principal)),
    )
    .await
    .unwrap();
    let listener = sema_translator::wire::bind(&socket).unwrap();
    let server = tokio::spawn(sema_translator::wire::serve_listener(
        listener,
        runtime.clone(),
    ));
    runtime.shutdown().await.unwrap();

    let socket_for_request = socket.clone();
    let reply = tokio::task::spawn_blocking(move || {
        exchange(
            &socket_for_request,
            request(AuthorityOperation::Read(ReadOperation::Current)),
            false,
        )
        .0
    })
    .await
    .unwrap();
    assert_eq!(
        reply,
        AuthorityReply::Rejected(NoWriteFailure::AuthorityUnavailable)
    );

    server.abort();
    let _ = server.await;
    if socket.exists() {
        std::fs::remove_file(socket).unwrap();
    }
}

#[test]
fn removing_a_seal_or_rename_receipt_prevents_process_readiness() {
    for remove_rename in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("missing-receipt.sock");
        let database = directory.path().join("missing-receipt.sema");
        let mut daemon = Daemon::start(&socket, &database);
        let base = current(&socket);
        let seal_key = key(if remove_rename { 21 } else { 11 });
        let seal = SealUniversal {
            operation_key: seal_key,
            expected: WritePrecondition {
                database_marker: base,
                table_generations: Vec::new(),
            },
            declarations: vec![DeclarationNode::Member(Name::new("ReceiptWitness"))],
            references: Vec::new(),
        };
        let sealed = exchange(
            &socket,
            request(AuthorityOperation::SealUniversal(seal)),
            true,
        )
        .0;
        let target = match sealed {
            AuthorityReply::Committed(CommittedReceipt::SealUniversal(receipt)) => {
                receipt.name_table.declarations()[0].encoded_id().clone()
            }
            other => panic!("expected seal receipt, got {other:?}"),
        };
        let removed_key = if remove_rename {
            let rename_key = key(22);
            let renamed = exchange(
                &socket,
                request(AuthorityOperation::Rename(Rename {
                    operation_key: rename_key,
                    expected: WritePrecondition {
                        database_marker: current(&socket),
                        table_generations: Vec::new(),
                    },
                    target,
                    new_spelling: Name::new("ReceiptRenamed"),
                })),
                true,
            )
            .0;
            assert!(matches!(
                renamed,
                AuthorityReply::Committed(CommittedReceipt::Rename(_))
            ));
            rename_key
        } else {
            seal_key
        };
        daemon.stop();

        let (engine, table) = open_fault_engine(&database);
        engine
            .retract(Retraction::new(table, receipt_record_key(removed_key)))
            .expect("retract isolated receipt");
        drop(engine);

        assert_refuses_readiness(&socket, &database);
    }
}

#[test]
fn corrupt_persisted_name_table_prevents_process_readiness() {
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("corrupt.sock");
    let database = directory.path().join("corrupt.sema");
    let (engine, table) = open_fault_engine(&database);
    let state = StoredAuthorityState {
        magic: *b"SEMATR01",
        archive_version: 1,
        roots: vec![
            RootRecord::encode_archive(VocabularyRoot::Universal).to_vec(),
            RootRecord::encode_archive(VocabularyRoot::Rust).to_vec(),
        ],
        name_table_archive: vec![0x5a],
        current_rust_version: RustVocabularyVersion::new(0),
        rust_releases: Vec::new(),
    };
    engine
        .assert(Assertion::new(table, StoredAuthorityRecord::State(state)))
        .expect("persist isolated corrupt bootstrap");
    drop(engine);

    assert_refuses_readiness(&socket, &database);
}
