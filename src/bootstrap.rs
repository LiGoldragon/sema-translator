//! Production authority and assembly boundary for bootstrap Ethos sources.
//!
//! The naming authority receives explicit opaque identity seats, canonical
//! identity bytes, and a complete textual-metadata transition. It never derives
//! an identity from source spelling or content and never allocates on behalf of
//! the reader. The source is planned first, the approved seats are matched to
//! exact declaration addresses, and `core-ethos` seals the resulting prepared
//! transaction through this authority's exact-draft receipt.

use std::collections::{BTreeMap, BTreeSet};

use core_ethos::bootstrap::{
    AssignedIdentity, BootstrapBuildError, BootstrapCatalog, BootstrapGrammarIdentities,
    BootstrapNamingAuthority, BootstrapNamingAuthorityRequest, BootstrapReadError,
    BootstrapReadPlan, BootstrapReader, BootstrapWriteError, DeclarationOccurrence,
    DeclarationPurpose, GeneratedStreamAssignment, GeneratedStreamAssignments, IdentityDisposition,
    NamingAssignment, NamingAssignments, PlannedScope, PreparedBootstrapDraft,
    PreparedBootstrapTransaction, TextualMetadataSnapshot, TextualMetadataTransition,
};
use name_table::Name;
use signal_sema_translator::{VocabularyEncodedId, VocabularyRoot};
use structural_codec::EncodedNameResolver;

fn rust_identity(local: u16) -> VocabularyEncodedId {
    VocabularyEncodedId::new(
        VocabularyRoot::Rust,
        vec![name_table::LocalEncodedId::new(local)],
    )
    .expect("authority-owned Rust vocabulary identities are nonempty")
}

/// Durable identity of one configured naming authority.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BootstrapAuthorityIdentity([u8; 32]);

impl BootstrapAuthorityIdentity {
    /// Wrap authority-owned identity bytes without interpreting their anatomy.
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Authority-owned identity bytes.
    pub const fn bytes(self) -> [u8; 32] {
        self.0
    }
}

/// Monotonic revision of an authority-approved bootstrap transition.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BootstrapAuthorityRevision(u64);

impl BootstrapAuthorityRevision {
    /// Construct an explicit authority revision.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Stored revision number.
    pub const fn value(self) -> u64 {
        self.0
    }
}

/// The fixed Rust vocabulary accepted by the bootstrap projection.
///
/// The authority owns its identity release. Consumers receive only this sealed
/// read view; they cannot create, amend, or substitute its vocabulary.
#[derive(Clone, Debug)]
pub struct SealedRustVocabulary {
    identities: [VocabularyEncodedId; 10],
    names: BTreeMap<VocabularyEncodedId, Name>,
}

/// A read position in the authority-sealed bootstrap Rust vocabulary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RustVocabularyTerm {
    NewtypeItem,
    EnumerationItem,
    Variant,
    TupleField,
    TypeReference,
    StructKeyword,
    EnumKeyword,
    PublicKeyword,
    Comma,
    Semicolon,
}

impl RustVocabularyTerm {
    const fn index(self) -> usize {
        match self {
            Self::NewtypeItem => 0,
            Self::EnumerationItem => 1,
            Self::Variant => 2,
            Self::TupleField => 3,
            Self::TypeReference => 4,
            Self::StructKeyword => 5,
            Self::EnumKeyword => 6,
            Self::PublicKeyword => 7,
            Self::Comma => 8,
            Self::Semicolon => 9,
        }
    }
}

impl SealedRustVocabulary {
    /// Read the one bootstrap Rust vocabulary released by this authority.
    pub fn bootstrap() -> Self {
        let locals = [
            37769, 61673, 64176, 16719, 16803, 52139, 13965, 64644, 44793, 4179,
        ];
        let spellings = [
            "NewtypeItemRecord",
            "EnumerationItemRecord",
            "VariantRecord",
            "TupleFieldRecord",
            "TypeReferenceRecord",
            "struct",
            "enum",
            "pub",
            ",",
            ";",
        ];
        let identities = locals.map(rust_identity);
        let names = identities
            .iter()
            .cloned()
            .zip(spellings)
            .map(|(identity, spelling)| (identity, Name::new(spelling)))
            .collect();
        Self { identities, names }
    }

    /// Read one authority-owned vocabulary identity.
    pub fn identity(&self, term: RustVocabularyTerm) -> &VocabularyEncodedId {
        &self.identities[term.index()]
    }
}

impl EncodedNameResolver<VocabularyRoot> for SealedRustVocabulary {
    fn resolve(&self, encoded_id: &VocabularyEncodedId) -> Option<&Name> {
        self.names.get(encoded_id)
    }
}

/// The two authority-supplied identities generated for one authored Stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorizedStreamSeats {
    initiation: VocabularyEncodedId,
    termination: VocabularyEncodedId,
}

impl AuthorizedStreamSeats {
    /// Record exact already-minted initiation and termination identities.
    pub const fn new(initiation: VocabularyEncodedId, termination: VocabularyEncodedId) -> Self {
        Self {
            initiation,
            termination,
        }
    }

    /// Exact initiation identity.
    pub const fn initiation(&self) -> &VocabularyEncodedId {
        &self.initiation
    }

    /// Exact termination identity.
    pub const fn termination(&self) -> &VocabularyEncodedId {
        &self.termination
    }
}

/// Explicit naming-authority approval for one before-to-after source mutation.
///
/// `new_identity_canonical_bytes` contains only identities already minted by
/// the authority. Its values are their authority-owned canonical projections;
/// no source byte participates in either identity or ordering.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorizedBootstrapTransition {
    after: TextualMetadataSnapshot,
    new_identity_canonical_bytes: BTreeMap<VocabularyEncodedId, Vec<u8>>,
    generated_streams: BTreeMap<VocabularyEncodedId, AuthorizedStreamSeats>,
}

impl AuthorizedBootstrapTransition {
    /// Construct one explicit approval. Exactness is checked against the source
    /// plan during assembly, so unused or missing authority seats are refused.
    pub fn new(
        after: TextualMetadataSnapshot,
        new_identity_canonical_bytes: BTreeMap<VocabularyEncodedId, Vec<u8>>,
        generated_streams: BTreeMap<VocabularyEncodedId, AuthorizedStreamSeats>,
    ) -> Self {
        Self {
            after,
            new_identity_canonical_bytes,
            generated_streams,
        }
    }

    /// Authority-approved after snapshot.
    pub const fn after(&self) -> &TextualMetadataSnapshot {
        &self.after
    }

    /// Canonical bytes for exactly the approved new identities.
    pub const fn new_identity_canonical_bytes(&self) -> &BTreeMap<VocabularyEncodedId, Vec<u8>> {
        &self.new_identity_canonical_bytes
    }

    /// Generated Stream seats keyed by the authored Stream output identity.
    pub const fn generated_streams(&self) -> &BTreeMap<VocabularyEncodedId, AuthorizedStreamSeats> {
        &self.generated_streams
    }
}

/// Proof object created only by the configured production assembler.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemaBootstrapAuthorityProof {
    authority: BootstrapAuthorityIdentity,
    revision: BootstrapAuthorityRevision,
}

/// Receipt binding an exact prepared draft to one authority configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemaBootstrapAuthorityReceipt {
    authority: BootstrapAuthorityIdentity,
    revision: BootstrapAuthorityRevision,
    draft: PreparedBootstrapDraft,
}

impl SemaBootstrapAuthorityReceipt {
    /// Authority that authenticated this exact draft.
    pub const fn authority(&self) -> BootstrapAuthorityIdentity {
        self.authority
    }

    /// Authority transition revision that authenticated this exact draft.
    pub const fn revision(&self) -> BootstrapAuthorityRevision {
        self.revision
    }
}

/// Naming authority injected into the strict bootstrap reader.
///
/// It authenticates only the configured explicit transition. It has no minting,
/// spelling lookup, or content-addressed allocation operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemaBootstrapNamingAuthority {
    identity: BootstrapAuthorityIdentity,
    revision: BootstrapAuthorityRevision,
    before: TextualMetadataSnapshot,
    approval: AuthorizedBootstrapTransition,
}

impl SemaBootstrapNamingAuthority {
    fn proof(&self) -> SemaBootstrapAuthorityProof {
        SemaBootstrapAuthorityProof {
            authority: self.identity,
            revision: self.revision,
        }
    }

    fn approves(&self, draft: &PreparedBootstrapDraft) -> bool {
        if draft.naming_transition.before() != &self.before
            || draft.naming_transition.after() != &self.approval.after
        {
            return false;
        }

        let observed_new = draft
            .identity_dispositions
            .iter()
            .filter_map(|(identity, disposition)| match disposition {
                IdentityDisposition::Existing => None,
                IdentityDisposition::New { canonical_bytes } => {
                    Some((identity.clone(), canonical_bytes.clone()))
                }
            })
            .collect::<BTreeMap<_, _>>();
        if observed_new != self.approval.new_identity_canonical_bytes {
            return false;
        }

        if draft.generated_streams.len() != self.approval.generated_streams.len() {
            return false;
        }
        draft.generated_streams.iter().all(|generated| {
            self.approval
                .generated_streams
                .get(&generated.output.name)
                .is_some_and(|approved| {
                    approved.initiation == generated.initiation.name
                        && approved.termination == generated.termination.name
                })
        })
    }
}

impl BootstrapNamingAuthority for SemaBootstrapNamingAuthority {
    type Proof = SemaBootstrapAuthorityProof;
    type Receipt = SemaBootstrapAuthorityReceipt;

    fn authorize(
        &self,
        request: BootstrapNamingAuthorityRequest<'_>,
        proof: &Self::Proof,
    ) -> Option<Self::Receipt> {
        (proof.authority == self.identity
            && proof.revision == self.revision
            && self.approves(request.transaction()))
        .then(|| SemaBootstrapAuthorityReceipt {
            authority: self.identity,
            revision: self.revision,
            draft: request.transaction().clone(),
        })
    }

    fn verify_receipt(
        &self,
        request: BootstrapNamingAuthorityRequest<'_>,
        receipt: &Self::Receipt,
    ) -> bool {
        receipt.authority == self.identity
            && receipt.revision == self.revision
            && receipt.draft == *request.transaction()
            && self.approves(request.transaction())
    }
}

/// Read-only name projection available only after transaction validation.
#[derive(Clone, Debug)]
pub struct VerifiedBootstrapResolver {
    authority: BootstrapAuthorityIdentity,
    revision: BootstrapAuthorityRevision,
    names: BTreeMap<VocabularyEncodedId, Name>,
}

impl VerifiedBootstrapResolver {
    fn from_validated(
        authority: BootstrapAuthorityIdentity,
        revision: BootstrapAuthorityRevision,
        snapshot: &TextualMetadataSnapshot,
    ) -> Self {
        Self {
            authority,
            revision,
            names: snapshot
                .records()
                .iter()
                .map(|record| {
                    (
                        record.encoded_name.clone(),
                        Name::new(record.address.visible_name.clone()),
                    )
                })
                .collect(),
        }
    }

    /// Authority whose validated receipt enabled this resolver.
    pub const fn authority(&self) -> BootstrapAuthorityIdentity {
        self.authority
    }

    /// Validated transition revision represented by this resolver.
    pub const fn revision(&self) -> BootstrapAuthorityRevision {
        self.revision
    }
}

impl EncodedNameResolver<VocabularyRoot> for VerifiedBootstrapResolver {
    fn resolve(&self, encoded_id: &VocabularyEncodedId) -> Option<&Name> {
        self.names.get(encoded_id)
    }
}

/// Opaque authority-sealed bootstrap meaning.
///
/// Only this authority boundary constructs it. Consumers can inspect the
/// validated transaction and its read-only projections, but cannot mint a
/// proof, receipt, seat, or replacement assembly.
pub struct AuthorizedBootstrap {
    reader: BootstrapReader<SemaBootstrapNamingAuthority>,
    transaction: PreparedBootstrapTransaction<SemaBootstrapNamingAuthority>,
    resolver: VerifiedBootstrapResolver,
    canonical_source: String,
}

impl AuthorizedBootstrap {
    /// Matching reader used to revalidate or canonically write the transaction.
    pub const fn reader(&self) -> &BootstrapReader<SemaBootstrapNamingAuthority> {
        &self.reader
    }

    /// Authority-branded prepared transaction accepted by the Nomos boundary.
    pub const fn transaction(&self) -> &PreparedBootstrapTransaction<SemaBootstrapNamingAuthority> {
        &self.transaction
    }

    /// Verified resolver for the exact transaction after-state.
    pub const fn resolver(&self) -> &VerifiedBootstrapResolver {
        &self.resolver
    }

    /// Canonical source projection produced after receipt validation.
    pub fn canonical_source(&self) -> &str {
        &self.canonical_source
    }
}

/// Production assembler configured with existing authority state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BootstrapTransactionAssembler {
    authority: BootstrapAuthorityIdentity,
    revision: BootstrapAuthorityRevision,
    grammar: BootstrapGrammarIdentities,
    catalog: BootstrapCatalog,
}

impl BootstrapTransactionAssembler {
    /// Seat the exact reader configuration and authority revision.
    pub const fn new(
        authority: BootstrapAuthorityIdentity,
        revision: BootstrapAuthorityRevision,
        grammar: BootstrapGrammarIdentities,
        catalog: BootstrapCatalog,
    ) -> Self {
        Self {
            authority,
            revision,
            grammar,
            catalog,
        }
    }

    /// Plan source, match every declaration to an approved opaque seat, seal
    /// the exact transaction, revalidate its receipt, and expose its resolver.
    pub fn assemble(
        &self,
        source: &str,
        approval: AuthorizedBootstrapTransition,
    ) -> Result<AuthorizedBootstrap, BootstrapAssemblyError> {
        let authority = SemaBootstrapNamingAuthority {
            identity: self.authority,
            revision: self.revision,
            before: self.catalog.metadata().clone(),
            approval,
        };
        let proof = authority.proof();
        let reader = BootstrapReader::build(
            self.grammar.clone(),
            self.catalog.clone(),
            authority.clone(),
        )?;
        let plan = reader.plan(source)?;
        let inputs = approved_inputs(&plan, &self.catalog, &authority.approval)?;
        let transaction = reader.seal(
            &plan,
            &inputs.assignments,
            &inputs.generated,
            &TextualMetadataTransition::new(
                self.catalog.metadata().clone(),
                authority.approval.after.clone(),
            ),
            &proof,
        )?;

        // `write` revalidates both the exact receipt and the complete prepared
        // model. The resolver constructor remains private and follows this gate.
        let canonical_source = reader.write(&transaction)?;
        let resolver = VerifiedBootstrapResolver::from_validated(
            self.authority,
            self.revision,
            transaction.naming_transition().after(),
        );
        Ok(AuthorizedBootstrap {
            reader,
            transaction,
            resolver,
            canonical_source,
        })
    }
}

struct ApprovedInputs {
    assignments: NamingAssignments,
    generated: GeneratedStreamAssignments,
}

fn approved_inputs(
    plan: &BootstrapReadPlan,
    catalog: &BootstrapCatalog,
    approval: &AuthorizedBootstrapTransition,
) -> Result<ApprovedInputs, BootstrapAssemblyError> {
    let mut by_occurrence = BTreeMap::<DeclarationOccurrence, VocabularyEncodedId>::new();
    let mut used_new = BTreeSet::new();
    let mut assignments = Vec::with_capacity(plan.declarations().len());

    for declaration in plan.declarations() {
        let owner = match declaration.scope() {
            PlannedScope::Module => None,
            PlannedScope::Enum(owner) | PlannedScope::Trait(owner) => {
                Some(by_occurrence.get(&owner).ok_or(
                    BootstrapAssemblyError::MissingAuthorizedLexicalOwner {
                        occurrence: declaration.occurrence().ordinal(),
                        owner: owner.ordinal(),
                    },
                )?)
            }
        };
        let identity = approval
            .after
            .identity_at(catalog.current_module_path(), owner, declaration.spelling())
            .ok_or_else(|| BootstrapAssemblyError::MissingAuthorizedProjection {
                occurrence: declaration.occurrence().ordinal(),
                spelling: declaration.spelling().to_owned(),
            })?
            .clone();
        let disposition = approved_disposition(
            &identity,
            catalog.metadata(),
            &approval.new_identity_canonical_bytes,
            &mut used_new,
        )?;
        by_occurrence.insert(declaration.occurrence(), identity.clone());
        assignments.push(NamingAssignment {
            occurrence: declaration.occurrence(),
            encoded_name: identity,
            disposition,
        });
    }

    let mut used_streams = BTreeSet::new();
    let mut generated = Vec::new();
    for declaration in plan
        .declarations()
        .iter()
        .filter(|declaration| declaration.purpose() == DeclarationPurpose::StreamInitiation)
    {
        let output = by_occurrence
            .get(&declaration.occurrence())
            .expect("every planned declaration was seated");
        let seats = approval.generated_streams.get(output).ok_or_else(|| {
            BootstrapAssemblyError::MissingAuthorizedStreamSeats {
                output: output.clone(),
            }
        })?;
        used_streams.insert(output.clone());
        generated.push(GeneratedStreamAssignment {
            source: declaration.occurrence(),
            initiation: AssignedIdentity {
                encoded_name: seats.initiation.clone(),
                disposition: approved_disposition(
                    &seats.initiation,
                    catalog.metadata(),
                    &approval.new_identity_canonical_bytes,
                    &mut used_new,
                )?,
            },
            termination: AssignedIdentity {
                encoded_name: seats.termination.clone(),
                disposition: approved_disposition(
                    &seats.termination,
                    catalog.metadata(),
                    &approval.new_identity_canonical_bytes,
                    &mut used_new,
                )?,
            },
        });
    }

    if let Some(identity) = approval
        .new_identity_canonical_bytes
        .keys()
        .find(|identity| !used_new.contains(*identity))
    {
        return Err(BootstrapAssemblyError::UnusedAuthorizedCanonicalBytes {
            identity: identity.clone(),
        });
    }
    if let Some(output) = approval
        .generated_streams
        .keys()
        .find(|output| !used_streams.contains(*output))
    {
        return Err(BootstrapAssemblyError::UnusedAuthorizedStreamSeats {
            output: output.clone(),
        });
    }

    Ok(ApprovedInputs {
        assignments: NamingAssignments::new(assignments)?,
        generated: GeneratedStreamAssignments::new(generated)?,
    })
}

fn approved_disposition(
    identity: &VocabularyEncodedId,
    before: &TextualMetadataSnapshot,
    approved_new: &BTreeMap<VocabularyEncodedId, Vec<u8>>,
    used_new: &mut BTreeSet<VocabularyEncodedId>,
) -> Result<IdentityDisposition, BootstrapAssemblyError> {
    if before.record(identity).is_some() {
        if approved_new.contains_key(identity) {
            return Err(
                BootstrapAssemblyError::CanonicalBytesSuppliedForExistingIdentity {
                    identity: identity.clone(),
                },
            );
        }
        return Ok(IdentityDisposition::Existing);
    }
    let canonical_bytes = approved_new.get(identity).ok_or_else(|| {
        BootstrapAssemblyError::MissingAuthorizedCanonicalBytes {
            identity: identity.clone(),
        }
    })?;
    used_new.insert(identity.clone());
    Ok(IdentityDisposition::New {
        canonical_bytes: canonical_bytes.clone(),
    })
}

/// Exact failure while assembling an authority-authenticated transaction.
#[derive(Debug, thiserror::Error)]
pub enum BootstrapAssemblyError {
    /// The shared structural reader could not be built.
    #[error(transparent)]
    Build(#[from] BootstrapBuildError),
    /// Source planning or transaction sealing was refused.
    #[error(transparent)]
    Read(#[from] BootstrapReadError),
    /// The validated transaction could not be canonically projected.
    #[error(transparent)]
    Write(#[from] BootstrapWriteError),
    /// A nested declaration appeared before its approved owner seat.
    #[error("declaration occurrence {occurrence} has no approved lexical-owner occurrence {owner}")]
    MissingAuthorizedLexicalOwner { occurrence: u32, owner: u32 },
    /// The after snapshot did not seat one exact planned declaration address.
    #[error("declaration occurrence {occurrence} ({spelling:?}) has no approved projection")]
    MissingAuthorizedProjection { occurrence: u32, spelling: String },
    /// A new opaque identity did not carry authority-owned canonical bytes.
    #[error("new identity {identity:?} has no approved canonical bytes")]
    MissingAuthorizedCanonicalBytes { identity: VocabularyEncodedId },
    /// Existing identity was incorrectly presented as a new allocation.
    #[error("existing identity {identity:?} was supplied new canonical bytes")]
    CanonicalBytesSuppliedForExistingIdentity { identity: VocabularyEncodedId },
    /// Approval included a new identity not consumed by this exact plan.
    #[error("canonical bytes for identity {identity:?} were not consumed by the source plan")]
    UnusedAuthorizedCanonicalBytes { identity: VocabularyEncodedId },
    /// An authored Stream had no explicit generated identities.
    #[error("Stream output {output:?} has no approved initiation/termination seats")]
    MissingAuthorizedStreamSeats { output: VocabularyEncodedId },
    /// Approval included generated Stream identities absent from this plan.
    #[error("generated Stream seats for output {output:?} were not consumed by the source plan")]
    UnusedAuthorizedStreamSeats { output: VocabularyEncodedId },
}
