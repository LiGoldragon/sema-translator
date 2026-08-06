//! The Sema-owned bootstrap naming authority.
//!
//! A caller supplies only source text and its source placement.  This module
//! owns every opaque name, canonical-order byte string, receipt, and staged
//! metadata change.  It intentionally exports none of those construction
//! capabilities.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, OnceLock};

use core_ethos::bootstrap::{
    BootstrapBuildError, BootstrapCatalog, BootstrapGrammarIdentities, BootstrapNamingAuthority,
    BootstrapNamingAuthorityRequest, BootstrapPriorIdentities, BootstrapPriorSlot,
    BootstrapPriorVocabulary, BootstrapReadError, BootstrapReadPlan, BootstrapReader,
    BootstrapVersionPolicy, BootstrapWriteError, CanonicalIdentityOrder, DeclarationOccurrence,
    EthosVersion, GeneratedStreamAssignments, IdentityDisposition, IdentitySchema,
    IdentitySchemaCatalog, NamingAssignment, NamingAssignments, PlannedScope,
    PreparedBootstrapDraft, PreparedBootstrapTransaction, TextualMetadataRecord,
    TextualMetadataSnapshot, TextualMetadataTransition, TextualProjectionAddress,
    bootstrap_prior_definitions,
};
use name_table::{EncodedName, TextualName, TrueName};

/// Caller-declared location for one source text.
///
/// The textual spelling of declarations is parsed from `source`; placement is
/// the only surrounding input accepted by the authority.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourcePlacement {
    module_path: Vec<String>,
    file_path: Vec<String>,
}

impl SourcePlacement {
    /// Record the module and file in which the source occurs.
    pub fn new(module_path: Vec<String>, file_path: Vec<String>) -> Self {
        Self {
            module_path,
            file_path,
        }
    }

    /// The source module path.
    pub fn module_path(&self) -> &[String] {
        &self.module_path
    }

    /// The source file placement, retained with the staged authority change.
    pub fn file_path(&self) -> &[String] {
        &self.file_path
    }
}

/// Sema's private bootstrap identity authority.
///
/// It mints opaque names with the operating system CSPRNG.  It has no API for
/// accepting caller-selected names, proofs, receipts, canonical bytes, seats,
/// catalogs, or preassembled transactions.
struct SemaBootstrapAuthority {
    grammar: BootstrapGrammarIdentities,
    seed: SeedAuthorityState,
    used_names: BTreeSet<EncodedName>,
    used_canonical_bytes: BTreeSet<Vec<u8>>,
    staged: BTreeMap<SourceRequest, StagedBootstrapChange>,
    replays: BTreeMap<SourceRequest, AuthorizedBootstrap>,
}

impl SemaBootstrapAuthority {
    /// Start a fresh authority from the identity-free core bootstrap seed.
    fn new() -> Result<Self, BootstrapAssemblyError> {
        let mut used_names = BTreeSet::new();
        let mut used_canonical_bytes = BTreeSet::new();
        let grammar = BootstrapGrammarIdentities {
            document: mint_name(&mut used_names)?,
            syntax: mint_name(&mut used_names)?,
        };
        let seed = SeedAuthorityState::mint(&mut used_names, &mut used_canonical_bytes)?;
        Ok(Self {
            grammar,
            seed,
            used_names,
            used_canonical_bytes,
            staged: BTreeMap::new(),
            replays: BTreeMap::new(),
        })
    }

    /// Plan, mint, seal, and stage one source-plus-placement request.
    ///
    /// Repeating an already realized request returns its exact realized
    /// authority result without minting. Distinct requests receive independent
    /// private stages; hqu.30 later owns their atomic durable installation.
    pub fn authorize(
        &mut self,
        source: &str,
        placement: SourcePlacement,
    ) -> Result<AuthorizedBootstrap, BootstrapAssemblyError> {
        let request = SourceRequest {
            source: source.to_owned(),
            placement,
        };
        if let Some(realized) = self.replays.get(&request) {
            return Ok(realized.clone());
        }
        let catalog = self.seed.catalog_for(&request.placement)?;
        let planning_reader = BootstrapReader::build(
            self.grammar.clone(),
            catalog.clone(),
            SemaNamingAuthority::unconfigured(catalog.metadata().clone()),
        )?;
        // `plan` refuses bundled/generated Streams before this authority has
        // minted any declaration identity or created a staged record.
        let plan = planning_reader.plan(&request.source)?;
        let allocation = self.allocate_for_plan(&plan, &catalog, &request.placement)?;
        let authority = SemaNamingAuthority::configured(
            catalog.metadata().clone(),
            allocation.after.clone(),
            allocation.new_canonical_bytes.clone(),
        );
        let proof = authority.proof();
        let reader = BootstrapReader::build(self.grammar.clone(), catalog.clone(), authority)?;
        let sealed_plan = reader.plan(&request.source)?;
        let assignments = allocation.assignments_for(&sealed_plan)?;
        let transaction = reader.seal(
            &sealed_plan,
            &assignments,
            &GeneratedStreamAssignments::new(Vec::new())?,
            &TextualMetadataTransition::new(catalog.metadata().clone(), allocation.after.clone()),
            &proof,
        )?;
        let canonical_source = reader.write(&transaction)?;
        let true_names = transaction.strict_value_true_names()?;
        let result = AuthorizedBootstrap {
            canonical_source,
            transaction,
        };
        let stage = StagedBootstrapChange {
            request: request.clone(),
            metadata: allocation.after,
            true_names,
            receipt: result.transaction.clone(),
        };
        debug_assert!(stage.is_consistent());
        self.staged.insert(request.clone(), stage);
        self.replays.insert(request, result.clone());
        Ok(result)
    }

    fn allocate_for_plan(
        &mut self,
        plan: &BootstrapReadPlan,
        catalog: &BootstrapCatalog,
        placement: &SourcePlacement,
    ) -> Result<Allocation, BootstrapAssemblyError> {
        let mut after_records = catalog.metadata().records().to_vec();
        let mut new_canonical_bytes = BTreeMap::new();
        let mut addresses = BTreeMap::new();
        let mut assigned = BTreeMap::new();

        for declaration in plan.declarations() {
            let owner: Option<EncodedName> = match declaration.scope() {
                PlannedScope::Module => None,
                PlannedScope::Enum(occurrence) | PlannedScope::Trait(occurrence) => assigned
                    .get(&occurrence)
                    .cloned()
                    .ok_or(BootstrapAssemblyError::MissingLexicalOwner {
                        occurrence: declaration.occurrence().ordinal(),
                        owner: occurrence.ordinal(),
                    })
                    .map(Some)?,
            };
            let key = AddressKey {
                owner,
                spelling: declaration.spelling().to_owned(),
            };
            let identity = catalog
                .metadata()
                .identity_at(
                    placement.module_path(),
                    owner.as_ref(),
                    declaration.spelling(),
                )
                .cloned()
                .or_else(|| addresses.get(&key).cloned());
            let identity = match identity {
                Some(identity) => identity,
                None => {
                    if catalog.metadata().records().iter().any(|record| {
                        record.address.textual_name.as_str() == declaration.spelling()
                            && (record.address.module_path != placement.module_path()
                                || record.address.lexical_owner != owner)
                    }) {
                        return Err(BootstrapAssemblyError::ImplicitRenameOrReparent {
                            spelling: declaration.spelling().to_owned(),
                        });
                    }
                    let identity = mint_name(&mut self.used_names)?;
                    let bytes = mint_canonical_bytes(&mut self.used_canonical_bytes)?;
                    new_canonical_bytes.insert(identity, bytes.clone());
                    after_records.push(TextualMetadataRecord {
                        address: TextualProjectionAddress {
                            module_path: placement.module_path().to_vec(),
                            lexical_owner: owner,
                            textual_name: TextualName::new(declaration.spelling()),
                        },
                        encoded_name: identity,
                    });
                    identity
                }
            };
            addresses.insert(key, identity);
            assigned.insert(declaration.occurrence(), identity);
        }

        let after = TextualMetadataSnapshot::new(after_records)?;
        Ok(Allocation {
            plan: plan
                .declarations()
                .iter()
                .map(|declaration| {
                    let identity = assigned[&declaration.occurrence()];
                    let disposition = if catalog.metadata().record(&identity).is_some() {
                        IdentityDisposition::Existing
                    } else {
                        IdentityDisposition::New {
                            canonical_bytes: new_canonical_bytes[&identity].clone(),
                        }
                    };
                    (declaration.occurrence(), identity, disposition)
                })
                .collect(),
            after,
            new_canonical_bytes,
        })
    }
}

/// Plan, mint, seal, and stage one source-plus-placement request.
///
/// This is the complete caller contract. The authority instance, its CSPRNG
/// allocation set, replay map, and pending stage are owned by Sema in this
/// process. hqu.30 is responsible for replacing the process-local stage with
/// the atomic durable persistence transition; callers never construct or pass
/// that state.
pub fn authorize_bootstrap(
    source: &str,
    placement: SourcePlacement,
) -> Result<AuthorizedBootstrap, BootstrapAssemblyError> {
    let state = authority_state();
    let mut authority = state
        .lock()
        .map_err(|_| BootstrapAssemblyError::AuthorityStatePoisoned)?;
    if authority.is_none() {
        *authority = Some(SemaBootstrapAuthority::new()?);
    }
    authority
        .as_mut()
        .expect("authority state was initialized above")
        .authorize(source, placement)
}

fn authority_state() -> &'static Mutex<Option<SemaBootstrapAuthority>> {
    static AUTHORITY_STATE: OnceLock<Mutex<Option<SemaBootstrapAuthority>>> = OnceLock::new();
    AUTHORITY_STATE.get_or_init(|| Mutex::new(None))
}

#[cfg(test)]
pub(crate) fn reset_authority_for_tests() {
    *authority_state()
        .lock()
        .expect("test authority mutex is not poisoned") = None;
}

/// An opaque, authority-sealed result.  Its receipt and transaction remain
/// private to Sema until the atomic persistence owner consumes the stage.
#[derive(Clone)]
pub struct AuthorizedBootstrap {
    canonical_source: String,
    transaction: PreparedBootstrapTransaction<SemaNamingAuthority>,
}

impl AuthorizedBootstrap {
    /// Canonical source projection after receipt validation.
    pub fn canonical_source(&self) -> &str {
        &self.canonical_source
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct SourceRequest {
    source: String,
    placement: SourcePlacement,
}

struct StagedBootstrapChange {
    request: SourceRequest,
    metadata: TextualMetadataSnapshot,
    true_names: BTreeMap<EncodedName, TrueName>,
    receipt: PreparedBootstrapTransaction<SemaNamingAuthority>,
}

impl StagedBootstrapChange {
    fn is_consistent(&self) -> bool {
        // This is deliberately a private readiness check, not an installer:
        // hqu.30 owns the atomic durable transition.  Reading all staged parts
        // here makes the invariant explicit without exporting any capability.
        !self.request.source.is_empty()
            && !self.request.placement.module_path().is_empty()
            && self.receipt.naming_transition().after() == &self.metadata
            && self
                .true_names
                .keys()
                .all(|identity| self.metadata.record(identity).is_some())
    }
}

#[derive(Clone)]
struct SeedAuthorityState {
    metadata: TextualMetadataSnapshot,
    schemas: IdentitySchemaCatalog,
    priors: BootstrapPriorVocabulary,
    canonical_order: CanonicalIdentityOrder,
}

impl SeedAuthorityState {
    fn mint(
        used_names: &mut BTreeSet<EncodedName>,
        used_canonical_bytes: &mut BTreeSet<Vec<u8>>,
    ) -> Result<Self, BootstrapAssemblyError> {
        let mut names = BTreeMap::new();
        let mut records = Vec::new();
        let mut schemas = Vec::new();
        let mut canonical_order = Vec::new();
        for definition in bootstrap_prior_definitions() {
            let identity = mint_name(used_names)?;
            names.insert(definition.slot, identity);
            records.push(TextualMetadataRecord {
                address: TextualProjectionAddress {
                    module_path: vec!["builtin".to_owned()],
                    lexical_owner: None,
                    textual_name: TextualName::new(definition.textual_name),
                },
                encoded_name: identity,
            });
            schemas.push(IdentitySchema::new(
                identity,
                definition.roles.iter().map(|role| role.schema_role()),
            )?);
            canonical_order.push((identity, mint_canonical_bytes(used_canonical_bytes)?));
        }
        let metadata = TextualMetadataSnapshot::new(records)?;
        let schemas = IdentitySchemaCatalog::new(schemas)?;
        let canonical_order = CanonicalIdentityOrder::new(canonical_order)?;
        let named = |slot| {
            *names
                .get(&slot)
                .expect("core seed contains every prior slot")
        };
        let priors = BootstrapPriorVocabulary::new(
            BootstrapPriorIdentities {
                interface_kind: named(BootstrapPriorSlot::InterfaceKind),
                nexus_kind: named(BootstrapPriorSlot::NexusKind),
                sema_kind: named(BootstrapPriorSlot::SemaKind),
                input_role: named(BootstrapPriorSlot::InputRole),
                output_role: named(BootstrapPriorSlot::OutputRole),
                refusal_role: named(BootstrapPriorSlot::RefusalRole),
                string_type: named(BootstrapPriorSlot::StringType),
                integer_type: named(BootstrapPriorSlot::IntegerType),
                boolean_type: named(BootstrapPriorSlot::BooleanType),
                unit_type: named(BootstrapPriorSlot::UnitType),
                vector_shape: named(BootstrapPriorSlot::VectorShape),
                option_shape: named(BootstrapPriorSlot::OptionShape),
                map_shape: named(BootstrapPriorSlot::MapShape),
                result_shape: named(BootstrapPriorSlot::ResultShape),
                stream_nomos: named(BootstrapPriorSlot::Stream),
                stream_shape: named(BootstrapPriorSlot::Stream),
                stream_identity_shape: named(BootstrapPriorSlot::StreamIdentityShape),
            },
            &schemas,
            &metadata,
        )?;
        Ok(Self {
            metadata,
            schemas,
            priors,
            canonical_order,
        })
    }

    fn catalog_for(
        &self,
        placement: &SourcePlacement,
    ) -> Result<BootstrapCatalog, BootstrapAssemblyError> {
        Ok(BootstrapCatalog::new(
            placement.module_path().to_vec(),
            self.metadata.clone(),
            self.schemas.clone(),
            self.priors.clone(),
            BootstrapVersionPolicy::exact(EthosVersion::new(1, 0, 0)),
            self.canonical_order.clone(),
        )?)
    }
}

#[derive(Clone)]
struct SemaNamingAuthority {
    before: TextualMetadataSnapshot,
    after: Option<TextualMetadataSnapshot>,
    new_canonical_bytes: BTreeMap<EncodedName, Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AuthorityProof;

#[derive(Clone, Debug, Eq, PartialEq)]
struct AuthorityReceipt(PreparedBootstrapDraft);

impl SemaNamingAuthority {
    fn unconfigured(before: TextualMetadataSnapshot) -> Self {
        Self {
            before,
            after: None,
            new_canonical_bytes: BTreeMap::new(),
        }
    }

    fn configured(
        before: TextualMetadataSnapshot,
        after: TextualMetadataSnapshot,
        new_canonical_bytes: BTreeMap<EncodedName, Vec<u8>>,
    ) -> Self {
        Self {
            before,
            after: Some(after),
            new_canonical_bytes,
        }
    }

    fn proof(&self) -> AuthorityProof {
        AuthorityProof
    }

    fn approves(&self, draft: &PreparedBootstrapDraft) -> bool {
        self.after.as_ref().is_some_and(|after| {
            draft.naming_transition.before() == &self.before
                && draft.naming_transition.after() == after
                && draft.generated_streams.is_empty()
                && draft
                    .identity_dispositions
                    .iter()
                    .filter_map(|(identity, disposition)| match disposition {
                        IdentityDisposition::Existing => None,
                        IdentityDisposition::New { canonical_bytes } => {
                            Some((*identity, canonical_bytes.clone()))
                        }
                    })
                    .collect::<BTreeMap<_, _>>()
                    == self.new_canonical_bytes
        })
    }
}

impl BootstrapNamingAuthority for SemaNamingAuthority {
    type Proof = AuthorityProof;
    type Receipt = AuthorityReceipt;

    fn authorize(
        &self,
        request: BootstrapNamingAuthorityRequest<'_>,
        _proof: &Self::Proof,
    ) -> Option<Self::Receipt> {
        self.approves(request.transaction())
            .then(|| AuthorityReceipt(request.transaction().clone()))
    }

    fn verify_receipt(
        &self,
        request: BootstrapNamingAuthorityRequest<'_>,
        receipt: &Self::Receipt,
    ) -> bool {
        receipt.0 == *request.transaction() && self.approves(request.transaction())
    }
}

#[derive(Clone)]
struct Allocation {
    plan: Vec<(DeclarationOccurrence, EncodedName, IdentityDisposition)>,
    after: TextualMetadataSnapshot,
    new_canonical_bytes: BTreeMap<EncodedName, Vec<u8>>,
}

impl Allocation {
    fn assignments_for(
        &self,
        plan: &BootstrapReadPlan,
    ) -> Result<NamingAssignments, BootstrapAssemblyError> {
        if self.plan.len() != plan.declarations().len() {
            return Err(BootstrapAssemblyError::PlanChangedDuringAuthorityAssembly);
        }
        NamingAssignments::new(
            self.plan
                .iter()
                .zip(plan.declarations())
                .map(|((planned, identity, disposition), declaration)| {
                    if planned.ordinal() != declaration.occurrence().ordinal() {
                        return Err(BootstrapAssemblyError::PlanChangedDuringAuthorityAssembly);
                    }
                    Ok(NamingAssignment {
                        occurrence: declaration.occurrence(),
                        encoded_name: *identity,
                        disposition: disposition.clone(),
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
        )
        .map_err(BootstrapAssemblyError::from)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct AddressKey {
    owner: Option<EncodedName>,
    spelling: String,
}

fn mint_name(used: &mut BTreeSet<EncodedName>) -> Result<EncodedName, BootstrapAssemblyError> {
    loop {
        let mut bytes = [0_u8; 16];
        getrandom::fill(&mut bytes).map_err(BootstrapAssemblyError::Entropy)?;
        let identity = EncodedName::from_archive_bytes(bytes);
        if used.insert(identity) {
            return Ok(identity);
        }
    }
}

fn mint_canonical_bytes(used: &mut BTreeSet<Vec<u8>>) -> Result<Vec<u8>, BootstrapAssemblyError> {
    loop {
        let mut bytes = vec![0_u8; 32];
        getrandom::fill(&mut bytes).map_err(BootstrapAssemblyError::Entropy)?;
        if used.insert(bytes.clone()) {
            return Ok(bytes);
        }
    }
}

/// Exact refusal while the Sema authority is planning or staging a source.
#[derive(Debug, thiserror::Error)]
pub enum BootstrapAssemblyError {
    /// The shared reader could not be configured from authority-owned state.
    #[error(transparent)]
    Build(#[from] BootstrapBuildError),
    /// Planning or sealing was structurally refused.
    #[error(transparent)]
    Read(#[from] BootstrapReadError),
    /// Canonical projection failed its reader validation.
    #[error(transparent)]
    Write(#[from] BootstrapWriteError),
    /// A direct strict declaration value could not be archived for its TrueName.
    #[error(transparent)]
    Archive(#[from] content_identity::ArchiveError),
    /// The operating-system CSPRNG did not provide entropy.
    #[error("the operating-system CSPRNG did not provide entropy: {0:?}")]
    Entropy(getrandom::Error),
    /// A nested declaration appeared before its owner occurrence.
    #[error("declaration occurrence {occurrence} has no lexical owner occurrence {owner}")]
    MissingLexicalOwner { occurrence: u32, owner: u32 },
    /// The same spelling at another occupied address would imply a rename or reparent.
    #[error(
        "{spelling:?} is already occupied at another projection address; explicit rename/reparent is required"
    )]
    ImplicitRenameOrReparent { spelling: String },
    /// The second reader plan differed from the allocation-free first plan.
    #[error("the source plan changed while the authority was assembling it")]
    PlanChangedDuringAuthorityAssembly,
    /// The Sema-owned authority state was poisoned by an earlier panic.
    #[error("the Sema-owned bootstrap authority state is unavailable")]
    AuthorityStatePoisoned,
}
