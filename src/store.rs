use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use name_table::{Declaration, Name, OperationKey, RootDeclaration, SealRequest, TableMutability};
use rkyv::{Archive, Deserialize, Serialize};
use sema_engine::{
    Assertion, Engine, EngineOpen, EngineRecord, FamilyDirectory, FamilyName, QueryPlan, RecordKey,
    RowMaterializer, SchemaHash, SchemaVersion, TableDescriptor, TableName, TableReference,
    VersionedStoreName, VersioningPolicy,
};
use signal_sema_translator::{
    AuthorityCapability, AuthorityOperation, AuthorityReply, AuthorityRequest,
    AuthorityRequestDigest, AuthorityRole, CommittedReceipt, CurrentAuthority, DatabaseMarker,
    NoWriteFailure, ObservedHead, ObservedSnapshot, PostCommitEvent, PrincipalId, ReadOperation,
    RenameCommitReceipt, RootRecord, RustVocabularyCommitReceipt, RustVocabularyVersion,
    SealCommitReceipt, VocabularyNameTable, VocabularyRoot,
};

use crate::{AuthorizationPolicy, DispatchOutcome};

const STATE_KEY: &str = "authority-state";
const ARCHIVE_MAGIC: [u8; 8] = *b"SEMATR01";
const ARCHIVE_VERSION: u16 = 1;
const AUTHORITY_TABLE: TableName = TableName::new("sema_translator_authority");
const AUTHORITY_FAMILY: &str = "sema-translator-authority";
const AUTHORITY_SCHEMA: &str = "sema-translator-authority-v1";
const BOOTSTRAP_OPERATION_KEY: OperationKey = OperationKey::new([
    0x53, 0x45, 0x4d, 0x41, 0x2d, 0x54, 0x52, 0x41, 0x4e, 0x53, 0x4c, 0x41, 0x54, 0x4f, 0x52, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
]);

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("embedded SEMA engine: {0}")]
    Engine(String),
    #[error("authority archive: {0}")]
    Archive(String),
    #[error("authority actor: {0}")]
    Actor(String),
    #[error(
        "authority marker prediction diverged: predicted {predicted:?}, committed {committed:?}"
    )]
    MarkerDivergence {
        predicted: DatabaseMarker,
        committed: DatabaseMarker,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Archive, Serialize, Deserialize, Clone, Debug)]
struct RustReleaseLedger {
    version: RustVocabularyVersion,
    digest: AuthorityRequestDigest,
}

#[derive(Archive, Serialize, Deserialize, Clone, Debug)]
struct AuthorityState {
    magic: [u8; 8],
    archive_version: u16,
    roots: Vec<Vec<u8>>,
    name_table_archive: Vec<u8>,
    current_rust_version: RustVocabularyVersion,
    rust_releases: Vec<RustReleaseLedger>,
}

#[derive(Archive, Serialize, Deserialize, Clone, Debug)]
enum AuthorityRecord {
    State(AuthorityState),
    Receipt(CommittedReceipt),
}

impl EngineRecord for AuthorityRecord {
    fn record_key(&self) -> RecordKey {
        match self {
            Self::State(_) => RecordKey::new(STATE_KEY),
            Self::Receipt(receipt) => RecordKey::new(receipt_record_key(receipt.operation_key())),
        }
    }
}

fn receipt_record_key(key: OperationKey) -> String {
    let mut output = String::with_capacity("receipt:".len() + 64);
    output.push_str("receipt:");
    for byte in key.bytes() {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

fn authority_table() -> TableReference<AuthorityRecord> {
    TableReference::new(AUTHORITY_TABLE)
}

fn authority_descriptor() -> TableDescriptor<AuthorityRecord> {
    TableDescriptor::new(
        AUTHORITY_TABLE,
        FamilyName::new(AUTHORITY_FAMILY),
        SchemaHash::for_label(AUTHORITY_SCHEMA),
    )
}

struct AuthorityDirectory;

impl FamilyDirectory for AuthorityDirectory {
    fn materialize(&self, row: RowMaterializer<'_>) -> sema_engine::Result<()> {
        row.apply(authority_table())
    }
}

pub(crate) struct AuthorityStore {
    engine: Engine,
    records: TableReference<AuthorityRecord>,
    names: VocabularyNameTable,
    rust_releases: Vec<RustReleaseLedger>,
    receipts: BTreeMap<OperationKey, CommittedReceipt>,
}

impl AuthorityStore {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let request = EngineOpen::new(path, SchemaVersion::new(1)).with_versioning(
            VersioningPolicy::new(VersionedStoreName::new("sema-translator")),
        );
        let mut engine = Engine::open_recovering(request, &AuthorityDirectory)
            .map_err(|error| Error::Engine(error.to_string()))?;
        let records = engine
            .register_table(authority_descriptor())
            .map_err(|error| Error::Engine(error.to_string()))?;
        if engine
            .current_database_marker()
            .map_err(|error| Error::Engine(error.to_string()))?
            .commit_sequence()
            .value()
            > 0
        {
            // The versioned log is the authoritative history. Refolding it
            // verifies its digest chain and completeness before any authority
            // row is trusted or the process can announce readiness.
            engine
                .rebuild_from_log(&AuthorityDirectory)
                .map_err(|error| Error::Engine(error.to_string()))?;
        }
        let existing = engine
            .match_records(QueryPlan::all(records))
            .map_err(|error| Error::Engine(error.to_string()))?
            .records()
            .to_vec();

        if existing.is_empty() {
            let state = bootstrap_state()?;
            engine
                .assert(Assertion::new(records, AuthorityRecord::State(state)))
                .map_err(|error| Error::Engine(error.to_string()))?;
        }

        let mut store = Self {
            engine,
            records,
            names: VocabularyNameTable::new(),
            rust_releases: Vec::new(),
            receipts: BTreeMap::new(),
        };
        store.reload_and_validate()?;
        Ok(store)
    }

    fn reload_and_validate(&mut self) -> Result<()> {
        let snapshot = self
            .engine
            .match_records(QueryPlan::all(self.records))
            .map_err(|error| Error::Engine(error.to_string()))?;
        let current_marker = marker_from_engine(snapshot.database_marker());
        let mut state = None;
        let mut receipts = BTreeMap::new();
        for record in snapshot.records() {
            match record {
                AuthorityRecord::State(found) => {
                    if state.replace(found.clone()).is_some() {
                        return Err(Error::Archive("duplicate authority state record".into()));
                    }
                }
                AuthorityRecord::Receipt(receipt) => {
                    if receipts
                        .insert(receipt.operation_key(), receipt.clone())
                        .is_some()
                    {
                        return Err(Error::Archive("duplicate external receipt".into()));
                    }
                }
            }
        }
        let state =
            state.ok_or_else(|| Error::Archive("authority state record is missing".into()))?;
        validate_state_envelope(&state)?;
        let names = VocabularyNameTable::from_archive_bytes(&state.name_table_archive)
            .map_err(|error| Error::Archive(error.to_string()))?;
        validate_rust_ledger(&state)?;
        validate_external_receipts(&self.engine, &names, &state, &receipts, current_marker)?;

        self.names = names;
        self.rust_releases = state.rust_releases;
        self.receipts = receipts;
        Ok(())
    }

    pub(crate) fn dispatch(
        &mut self,
        authenticated: PrincipalId,
        request: AuthorityRequest,
        authorization: &dyn AuthorizationPolicy,
    ) -> Result<DispatchOutcome> {
        let (role, capability) = required_authority(&request.operation);
        if !authorization.allows(authenticated, &request.authorization, role, capability) {
            return Ok(DispatchOutcome::rejected(
                NoWriteFailure::AuthorizationDenied {
                    principal: authenticated,
                    role: request.authorization.role,
                    capability: request.authorization.capability,
                },
            ));
        }

        match request.operation {
            AuthorityOperation::SealUniversal(operation) => self.seal(operation),
            AuthorityOperation::Rename(operation) => self.rename(operation),
            AuthorityOperation::PublishRustVocabulary(operation) => {
                self.publish_rust_vocabulary(operation)
            }
            AuthorityOperation::Read(operation) => self.read(operation),
        }
    }

    fn seal(
        &mut self,
        operation: signal_sema_translator::SealUniversal,
    ) -> Result<DispatchOutcome> {
        let request_digest = match operation.canonical_request_digest() {
            Ok(digest) => digest,
            Err(error) => return Ok(DispatchOutcome::rejected(error)),
        };
        let lowered = operation.lower();

        if let Some(replay) = self.idempotent_replay(operation.operation_key, request_digest) {
            return Ok(replay);
        }
        if let Some(failure) = self.check_precondition(&operation.expected)? {
            return Ok(DispatchOutcome::rejected(failure));
        }

        let mut next_names = self.names.clone();
        let name_table = match next_names.seal(lowered) {
            Ok(receipt) => receipt,
            Err(error) => return Ok(DispatchOutcome::rejected(error.into())),
        };
        let predicted = self.predict_next_marker()?;
        let receipt = CommittedReceipt::SealUniversal(SealCommitReceipt {
            request_digest,
            name_table,
            database_marker: predicted,
        });
        if let Err(error) = self.commit_state(
            next_names,
            self.rust_releases.clone(),
            receipt.clone(),
            predicted,
        ) {
            return commit_failure(error);
        }
        let CommittedReceipt::SealUniversal(committed) = receipt.clone() else {
            unreachable!()
        };
        Ok(DispatchOutcome::committed(
            receipt,
            PostCommitEvent::UniversalSealed(committed),
        ))
    }

    fn rename(&mut self, operation: signal_sema_translator::Rename) -> Result<DispatchOutcome> {
        let request_digest = rename_digest(&operation);
        if let Some(replay) = self.idempotent_replay(operation.operation_key, request_digest) {
            return Ok(replay);
        }
        if let Some(failure) = self.check_precondition(&operation.expected)? {
            return Ok(DispatchOutcome::rejected(failure));
        }

        let mut next_names = self.names.clone();
        let name_table = match next_names.rename(operation.lower()) {
            Ok(receipt) => receipt,
            Err(error) => return Ok(DispatchOutcome::rejected(error.into())),
        };
        let predicted = self.predict_next_marker()?;
        let receipt = CommittedReceipt::Rename(RenameCommitReceipt {
            request_digest,
            name_table,
            database_marker: predicted,
        });
        if let Err(error) = self.commit_state(
            next_names,
            self.rust_releases.clone(),
            receipt.clone(),
            predicted,
        ) {
            return commit_failure(error);
        }
        let CommittedReceipt::Rename(committed) = receipt.clone() else {
            unreachable!()
        };
        Ok(DispatchOutcome::committed(
            receipt,
            PostCommitEvent::Renamed(committed),
        ))
    }

    fn publish_rust_vocabulary(
        &mut self,
        operation: signal_sema_translator::RustVocabularyRelease,
    ) -> Result<DispatchOutcome> {
        let lowered = operation.lower();
        let generic_digest = match self.names.request_digest(&lowered) {
            Ok(digest) => digest,
            Err(error) => return Ok(DispatchOutcome::rejected(error.into())),
        };
        let request_digest = rust_release_digest(operation.version, generic_digest.bytes());
        if let Some(replay) = self.idempotent_replay(operation.operation_key, request_digest) {
            return Ok(replay);
        }
        if let Some(failure) = self.check_precondition(&operation.expected)? {
            return Ok(DispatchOutcome::rejected(failure));
        }
        if let Some(current) = self.rust_releases.last() {
            if operation.version <= current.version {
                if self
                    .rust_releases
                    .iter()
                    .find(|entry| entry.version == operation.version)
                    .is_some_and(|entry| entry.digest != request_digest)
                {
                    return Ok(DispatchOutcome::rejected(
                        NoWriteFailure::RustVocabularyVersionConflict {
                            version: operation.version,
                        },
                    ));
                }
                return Ok(DispatchOutcome::rejected(
                    NoWriteFailure::StaleRustVocabularyVersion {
                        submitted: operation.version,
                        current: current.version,
                    },
                ));
            }
        } else if operation.version.value() == 0 {
            return Ok(DispatchOutcome::rejected(
                NoWriteFailure::StaleRustVocabularyVersion {
                    submitted: operation.version,
                    current: RustVocabularyVersion::new(0),
                },
            ));
        }

        let mut next_names = self.names.clone();
        let name_table = match next_names.seal(lowered) {
            Ok(receipt) => receipt,
            Err(error) => return Ok(DispatchOutcome::rejected(error.into())),
        };
        let mut next_releases = self.rust_releases.clone();
        next_releases.push(RustReleaseLedger {
            version: operation.version,
            digest: request_digest,
        });
        let predicted = self.predict_next_marker()?;
        let receipt = CommittedReceipt::PublishRustVocabulary(RustVocabularyCommitReceipt {
            version: operation.version,
            request_digest,
            name_table,
            database_marker: predicted,
        });
        if let Err(error) = self.commit_state(next_names, next_releases, receipt.clone(), predicted)
        {
            return commit_failure(error);
        }
        let CommittedReceipt::PublishRustVocabulary(committed) = receipt.clone() else {
            unreachable!()
        };
        Ok(DispatchOutcome::committed(
            receipt,
            PostCommitEvent::RustVocabularyPublished(committed),
        ))
    }

    fn read(&self, operation: ReadOperation) -> Result<DispatchOutcome> {
        let marker = marker_from_engine(
            self.engine
                .current_database_marker()
                .map_err(|error| Error::Engine(error.to_string()))?,
        );
        let reply = match operation {
            ReadOperation::Current => AuthorityReply::Current(CurrentAuthority {
                database_marker: marker,
                name_table_revision: self.names.revision(),
            }),
            ReadOperation::Head { address } => match self.names.head(&address) {
                Some(head) => AuthorityReply::Head(ObservedHead {
                    database_marker: marker,
                    head: head.clone(),
                }),
                None => AuthorityReply::Rejected(NoWriteFailure::UnknownTable { address }),
            },
            ReadOperation::CurrentSnapshot { address } => {
                match self.names.current_snapshot(&address) {
                    Ok(snapshot) => AuthorityReply::Snapshot(ObservedSnapshot {
                        database_marker: marker,
                        snapshot: snapshot.clone(),
                    }),
                    Err(error) => AuthorityReply::Rejected(error.into()),
                }
            }
            ReadOperation::HistoricalSnapshot { address, snapshot } => {
                match self.names.snapshot(snapshot) {
                    Some(found) if found.address() == &address => {
                        AuthorityReply::Snapshot(ObservedSnapshot {
                            database_marker: marker,
                            snapshot: found.clone(),
                        })
                    }
                    _ => AuthorityReply::Rejected(NoWriteFailure::SnapshotIntegrityMismatch {
                        digest: snapshot,
                    }),
                }
            }
            ReadOperation::CommittedReceipt { operation_key } => {
                match self.receipts.get(&operation_key) {
                    Some(receipt) => AuthorityReply::Receipt(receipt.clone()),
                    None => AuthorityReply::Rejected(NoWriteFailure::UnknownCommittedReceipt {
                        operation_key,
                    }),
                }
            }
        };
        Ok(DispatchOutcome { reply, event: None })
    }

    fn idempotent_replay(
        &self,
        operation_key: OperationKey,
        digest: AuthorityRequestDigest,
    ) -> Option<DispatchOutcome> {
        self.receipts.get(&operation_key).map(|receipt| {
            if receipt.request_digest() == digest {
                DispatchOutcome {
                    reply: AuthorityReply::Committed(receipt.clone()),
                    event: None,
                }
            } else {
                DispatchOutcome::rejected(NoWriteFailure::IdempotencyConflict { operation_key })
            }
        })
    }

    fn check_precondition(
        &self,
        expected: &signal_sema_translator::WritePrecondition,
    ) -> Result<Option<NoWriteFailure>> {
        let actual = self
            .engine
            .current_database_marker()
            .map(marker_from_engine)
            .map_err(|error| Error::Engine(error.to_string()))?;
        if expected.database_marker != actual {
            return Ok(Some(NoWriteFailure::StaleDatabaseMarker {
                expected: expected.database_marker,
                actual,
            }));
        }
        let mut addresses = BTreeSet::new();
        for table in &expected.table_generations {
            if !addresses.insert(table.address.clone()) {
                return Ok(Some(NoWriteFailure::DuplicateExpectedTable {
                    address: table.address.clone(),
                }));
            }
            let Some(head) = self.names.head(&table.address) else {
                return Ok(Some(NoWriteFailure::UnknownTable {
                    address: table.address.clone(),
                }));
            };
            let actual = head.generation().value();
            if table.generation != actual {
                return Ok(Some(NoWriteFailure::StaleTableGeneration {
                    address: table.address.clone(),
                    expected: table.generation,
                    actual,
                }));
            }
        }
        Ok(None)
    }

    fn predict_next_marker(&self) -> Result<DatabaseMarker> {
        let current = marker_from_engine(
            self.engine
                .current_database_marker()
                .map_err(|error| Error::Engine(error.to_string()))?,
        );
        current
            .checked_successor()
            .ok_or_else(|| Error::Engine("database marker exhausted".into()))
    }

    fn commit_state(
        &mut self,
        next_names: VocabularyNameTable,
        next_releases: Vec<RustReleaseLedger>,
        receipt: CommittedReceipt,
        predicted: DatabaseMarker,
    ) -> Result<()> {
        let state = persisted_state(&next_names, next_releases.clone())?;
        let commit = self
            .engine
            .begin_atomic_commit()
            .mutate(self.records, AuthorityRecord::State(state))
            .assert(self.records, AuthorityRecord::Receipt(receipt.clone()));
        let committed = self
            .engine
            .commit_atomic(commit)
            .map_err(|error| Error::Engine(error.to_string()))?;
        let committed = DatabaseMarker::new(
            committed.commit_sequence().value(),
            committed.snapshot().value(),
        );
        if committed != predicted {
            return Err(Error::MarkerDivergence {
                predicted,
                committed,
            });
        }
        self.names = next_names;
        self.rust_releases = next_releases;
        self.receipts.insert(receipt.operation_key(), receipt);
        Ok(())
    }
}

fn commit_failure(error: Error) -> Result<DispatchOutcome> {
    match error {
        Error::Engine(_) => Ok(DispatchOutcome::rejected(NoWriteFailure::CommitFailed {
            message: "embedded database rejected the atomic authority commit".into(),
        })),
        Error::Archive(_) => Ok(DispatchOutcome::rejected(
            NoWriteFailure::SerializationFailed {
                message: "authority state could not be serialized".into(),
            },
        )),
        fatal => Err(fatal),
    }
}

fn required_authority(operation: &AuthorityOperation) -> (AuthorityRole, AuthorityCapability) {
    match operation {
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
    }
}

fn marker_from_engine(marker: sema_engine::DatabaseMarker) -> DatabaseMarker {
    DatabaseMarker::new(marker.commit_sequence().value(), marker.snapshot().value())
}

fn expected_root_records() -> Vec<Vec<u8>> {
    vec![
        RootRecord::encode_archive(VocabularyRoot::Universal).to_vec(),
        RootRecord::encode_archive(VocabularyRoot::Rust).to_vec(),
    ]
}

fn persisted_state(
    names: &VocabularyNameTable,
    rust_releases: Vec<RustReleaseLedger>,
) -> Result<AuthorityState> {
    let name_table_archive = names
        .to_archive_bytes()
        .map_err(|error| Error::Archive(error.to_string()))?
        .to_vec();
    let current_rust_version = rust_releases
        .last()
        .map_or(RustVocabularyVersion::new(0), |entry| entry.version);
    Ok(AuthorityState {
        magic: ARCHIVE_MAGIC,
        archive_version: ARCHIVE_VERSION,
        roots: expected_root_records(),
        name_table_archive,
        current_rust_version,
        rust_releases,
    })
}

fn bootstrap_state() -> Result<AuthorityState> {
    let mut names = VocabularyNameTable::new();
    let request = SealRequest::new(
        BOOTSTRAP_OPERATION_KEY,
        vec![
            RootDeclaration::new(
                VocabularyRoot::Universal,
                TableMutability::Mutable,
                vec![
                    Declaration::Member(Name::new("Integer")),
                    Declaration::Member(Name::new("String")),
                ],
            ),
            RootDeclaration::new(
                VocabularyRoot::Rust,
                TableMutability::Immutable,
                vec![Declaration::Member(Name::new("u64"))],
            ),
        ],
        Vec::new(),
    );
    names
        .seal(request)
        .map_err(|error| Error::Archive(error.to_string()))?;
    persisted_state(&names, Vec::new())
}

fn validate_state_envelope(state: &AuthorityState) -> Result<()> {
    if state.magic != ARCHIVE_MAGIC {
        return Err(Error::Archive(
            "legacy or incompatible authority archive marker".into(),
        ));
    }
    if state.archive_version != ARCHIVE_VERSION {
        return Err(Error::Archive(format!(
            "unsupported authority archive version {}",
            state.archive_version
        )));
    }
    if state.roots != expected_root_records() {
        return Err(Error::Archive(
            "authority archive does not contain exactly Universal and Rust roots".into(),
        ));
    }
    for encoded in &state.roots {
        RootRecord::decode_archive(encoded).map_err(|error| Error::Archive(error.to_string()))?;
    }
    Ok(())
}

fn validate_rust_ledger(state: &AuthorityState) -> Result<()> {
    let mut previous = 0;
    for (index, release) in state.rust_releases.iter().enumerate() {
        let value = release.version.value();
        if value == 0 || (index > 0 && value <= previous) {
            return Err(Error::Archive(
                "Rust vocabulary release versions are not strictly monotonic".into(),
            ));
        }
        previous = value;
    }
    let expected = state
        .rust_releases
        .last()
        .map_or(0, |release| release.version.value());
    if state.current_rust_version.value() != expected {
        return Err(Error::Archive(
            "Rust vocabulary head disagrees with its release ledger".into(),
        ));
    }
    Ok(())
}

fn validate_external_receipts(
    engine: &Engine,
    names: &VocabularyNameTable,
    state: &AuthorityState,
    receipts: &BTreeMap<OperationKey, CommittedReceipt>,
    current: DatabaseMarker,
) -> Result<()> {
    let log = engine
        .commit_log()
        .map_err(|error| Error::Engine(error.to_string()))?;
    let versioned_log = engine
        .versioned_commit_log()
        .map_err(|error| Error::Engine(error.to_string()))?;
    if log.len() != versioned_log.len() {
        return Err(Error::Archive(
            "metadata and versioned authority histories differ in length".into(),
        ));
    }
    let mut previous_versioned_digest = None;
    for (metadata, versioned) in log.iter().zip(&versioned_log) {
        if metadata.commit_sequence() != versioned.commit_sequence()
            || metadata.snapshot() != versioned.snapshot()
            || metadata.operation_count() != versioned.operation_count()
            || versioned.previous_entry_digest() != previous_versioned_digest
        {
            return Err(Error::Archive(
                "metadata and versioned authority histories disagree".into(),
            ));
        }
        for (metadata_operation, versioned_operation) in
            metadata.operations().iter().zip(versioned.operations())
        {
            if metadata_operation.operation().as_record_head()
                != versioned_operation.operation().as_record_head()
                || metadata_operation.table_name() != versioned_operation.table_name()
                || metadata_operation.key() != versioned_operation.key()
            {
                return Err(Error::Archive(
                    "metadata and versioned authority operations disagree".into(),
                ));
            }
        }
        previous_versioned_digest = Some(versioned.entry_digest());
    }
    if engine
        .versioned_chain_head()
        .map_err(|error| Error::Engine(error.to_string()))?
        != previous_versioned_digest
    {
        return Err(Error::Archive(
            "verified authority history disagrees with its chain head".into(),
        ));
    }
    let bootstrap_bytes = versioned_log
        .first()
        .and_then(|entry| entry.operations().head().payload().bytes())
        .ok_or_else(|| Error::Archive("authority bootstrap payload is missing".into()))?;
    let bootstrap_record =
        rkyv::from_bytes::<AuthorityRecord, rkyv::rancor::Error>(bootstrap_bytes)
            .map_err(|error| Error::Archive(format!("authority bootstrap payload: {error}")))?;
    let AuthorityRecord::State(initial_state) = bootstrap_record else {
        return Err(Error::Archive(
            "authority history does not begin with its state bootstrap".into(),
        ));
    };
    validate_state_envelope(&initial_state)?;
    validate_rust_ledger(&initial_state)?;
    let initial_names = VocabularyNameTable::from_archive_bytes(&initial_state.name_table_archive)
        .map_err(|error| Error::Archive(error.to_string()))?;
    let expected_initial_state = bootstrap_state()?;
    let expected_initial_names =
        VocabularyNameTable::from_archive_bytes(&expected_initial_state.name_table_archive)
            .map_err(|error| Error::Archive(error.to_string()))?;
    if initial_names != expected_initial_names || !initial_state.rust_releases.is_empty() {
        return Err(Error::Archive(
            "authority history does not begin with the fixed production bootstrap".into(),
        ));
    }

    let present_receipts = receipts
        .iter()
        .map(|(key, receipt)| (receipt_record_key(*key), receipt))
        .collect::<BTreeMap<_, _>>();
    let mut seen_receipt_keys = BTreeSet::new();
    let mut bootstrap_seen = false;
    let mut previous_sequence = 0;
    let mut previous_snapshot = 0;
    for (index, commit) in log.iter().enumerate() {
        let sequence = commit.commit_sequence().value();
        let snapshot = commit.snapshot().value();
        if sequence != previous_sequence + 1 || snapshot != previous_snapshot + 1 {
            return Err(Error::Archive(
                "authority commit markers are not one contiguous ancestry".into(),
            ));
        }
        previous_sequence = sequence;
        previous_snapshot = snapshot;

        let mut state_operations = Vec::new();
        let mut receipt_operations = Vec::new();
        for operation in commit.operations() {
            if operation.table_name() != AUTHORITY_TABLE.as_str() {
                return Err(Error::Archive(
                    "owned authority database contains a foreign record family".into(),
                ));
            }
            let key = operation
                .key()
                .ok_or_else(|| {
                    Error::Archive("authority history contains an unkeyed write".into())
                })?
                .to_owned_string();
            if key == STATE_KEY {
                state_operations.push(operation);
            } else if key.starts_with("receipt:") {
                receipt_operations.push((key, operation));
            } else {
                return Err(Error::Archive(
                    "authority history contains an unknown record key".into(),
                ));
            }
        }

        match (state_operations.as_slice(), receipt_operations.as_slice()) {
            ([state_operation], []) => {
                if index != 0
                    || bootstrap_seen
                    || commit.operation_count() != 1
                    || state_operation.operation().as_record_head() != "Assert"
                {
                    return Err(Error::Archive(
                        "receiptless authority mutation is not the unique bootstrap".into(),
                    ));
                }
                bootstrap_seen = true;
            }
            ([state_operation], [(receipt_key, receipt_operation)]) => {
                if !bootstrap_seen
                    || commit.operation_count() != 2
                    || state_operation.operation().as_record_head() != "Mutate"
                    || receipt_operation.operation().as_record_head() != "Assert"
                    || !seen_receipt_keys.insert(receipt_key.clone())
                {
                    return Err(Error::Archive(
                        "authority mutation is not one state transition plus one new receipt"
                            .into(),
                    ));
                }
                let receipt = present_receipts.get(receipt_key).ok_or_else(|| {
                    Error::Archive("authority mutation is missing its external receipt".into())
                })?;
                if receipt.database_marker() != DatabaseMarker::new(sequence, snapshot) {
                    return Err(Error::Archive(
                        "authority receipt marker disagrees with its committing transition".into(),
                    ));
                }
            }
            _ => {
                return Err(Error::Archive(
                    "authority commit does not have the required state-and-receipt shape".into(),
                ));
            }
        }
    }
    if !bootstrap_seen
        || seen_receipt_keys.len() != present_receipts.len()
        || DatabaseMarker::new(previous_sequence, previous_snapshot) != current
    {
        return Err(Error::Archive(
            "authority history is incomplete at its current marker".into(),
        ));
    }

    let mut markers = BTreeSet::new();
    for (key, receipt) in receipts {
        if receipt.operation_key() != *key {
            return Err(Error::Archive(
                "external receipt key disagrees with its record key".into(),
            ));
        }
        let marker = receipt.database_marker();
        if marker > current || !markers.insert(marker) {
            return Err(Error::Archive(
                "external receipt marker is future or duplicated".into(),
            ));
        }
        let expected_record_key = receipt_record_key(*key);
        let matching_commit = log.iter().find(|entry| {
            entry.commit_sequence().value() == marker.commit_sequence
                && entry.snapshot().value() == marker.snapshot
        });
        let Some(commit) = matching_commit else {
            return Err(Error::Archive(
                "external receipt has no matching engine commit".into(),
            ));
        };
        let keys = commit
            .operations()
            .iter()
            .filter_map(|operation| operation.key())
            .map(RecordKey::to_owned_string)
            .collect::<BTreeSet<_>>();
        if !keys.contains(STATE_KEY) || !keys.contains(&expected_record_key) {
            return Err(Error::Archive(
                "receipt and authority state were not committed together".into(),
            ));
        }

        match receipt {
            CommittedReceipt::SealUniversal(found) => {
                validate_seal_receipt(names, &found.name_table)?;
            }
            CommittedReceipt::Rename(found) => {
                if found.name_table.resulting_revision() > names.revision() {
                    return Err(Error::Archive(
                        "rename receipt names a future state revision".into(),
                    ));
                }
            }
            CommittedReceipt::PublishRustVocabulary(found) => {
                validate_seal_receipt(names, &found.name_table)?;
                let ledger = state
                    .rust_releases
                    .iter()
                    .find(|entry| entry.version == found.version)
                    .ok_or_else(|| {
                        Error::Archive("Rust receipt is absent from release ledger".into())
                    })?;
                if ledger.digest != found.request_digest {
                    return Err(Error::Archive(
                        "Rust receipt digest disagrees with release ledger".into(),
                    ));
                }
            }
        }
    }
    if state.rust_releases.len()
        != receipts
            .values()
            .filter(|receipt| matches!(receipt, CommittedReceipt::PublishRustVocabulary(_)))
            .count()
    {
        return Err(Error::Archive(
            "Rust release ledger and external receipts differ".into(),
        ));
    }
    Ok(())
}

fn validate_seal_receipt(
    names: &VocabularyNameTable,
    receipt: &signal_sema_translator::VocabularySealReceipt,
) -> Result<()> {
    if receipt.resulting_revision() > names.revision() {
        return Err(Error::Archive(
            "seal receipt names a future state revision".into(),
        ));
    }
    for change in receipt.changed_tables() {
        let snapshot = names.snapshot(change.snapshot()).ok_or_else(|| {
            Error::Archive("seal receipt snapshot is absent from authority state".into())
        })?;
        if snapshot.address() != change.address()
            || snapshot.generation() != change.generation()
            || snapshot.digest() != change.snapshot()
        {
            return Err(Error::Archive(
                "seal receipt snapshot ancestry is inconsistent".into(),
            ));
        }
    }
    Ok(())
}

fn rust_release_digest(
    version: RustVocabularyVersion,
    generic: &[u8; 32],
) -> AuthorityRequestDigest {
    let mut hasher = blake3::Hasher::new();
    let prefix = b"sema-translator/rust-vocabulary-release/v1";
    hasher.update(&(prefix.len() as u64).to_le_bytes());
    hasher.update(prefix);
    hasher.update(&version.value().to_le_bytes());
    hasher.update(generic);
    AuthorityRequestDigest::new(*hasher.finalize().as_bytes())
}

fn rename_digest(operation: &signal_sema_translator::Rename) -> AuthorityRequestDigest {
    let mut hasher = blake3::Hasher::new();
    let prefix = b"sema-translator/rename/v1";
    hasher.update(&(prefix.len() as u64).to_le_bytes());
    hasher.update(prefix);
    hasher.update(&[operation.target.root_variant().tag()]);
    hasher.update(&(operation.target.chain().len() as u64).to_le_bytes());
    for local in operation.target.chain() {
        hasher.update(&local.value().to_le_bytes());
    }
    hasher.update(&(operation.new_spelling.as_bytes().len() as u64).to_le_bytes());
    hasher.update(operation.new_spelling.as_bytes());
    AuthorityRequestDigest::new(*hasher.finalize().as_bytes())
}
