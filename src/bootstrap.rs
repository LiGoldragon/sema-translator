//! The Sema-owned bootstrap naming authority.
//!
//! A caller supplies only source text and its source placement.  This module
//! owns every opaque name, canonical-order byte string, receipt, and staged
//! metadata change.  It intentionally exports none of those construction
//! capabilities.

use std::collections::{BTreeMap, BTreeSet};

use core_ethos::bootstrap::{
    AdmittedToCoreEthosRawConstruction, BootstrapBuildError, BootstrapCatalog,
    BootstrapCatalogMetadata, BootstrapGrammarIdentities, BootstrapNamingAuthority,
    BootstrapNamingAuthorityRequest, BootstrapPriorIdentities, BootstrapPriorSlot,
    BootstrapPriorVocabulary, BootstrapReadError, BootstrapReadPlan, BootstrapReader,
    BootstrapVersionPolicy, BootstrapWriteError, CanonicalIdentityOrder, DeclarationOccurrence,
    EthosVersion, IdentityDisposition, IdentitySchema, IdentitySchemaCatalog, NamingAssignments,
    PlannedScope, PreparedBootstrapDraft, PreparedBootstrapTransaction, SemaTransactionAssembler,
    TextualMetadataRecord, TextualMetadataSnapshot, TextualMetadataTransition,
    TextualProjectionAddress, bootstrap_prior_definitions,
};
use name_table::{
    EncodedName, FilePlacement, ModulePlacement, NameAssociation, NameView, TextualMetadata,
    TextualName, TrueName,
};
use structural_codec::EncodedNameResolver;

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

/// An empty handle to one private Sema bootstrap authority world.
///
/// It mints opaque names with the operating system CSPRNG.  It has no API for
/// accepting caller-selected names, proofs, receipts, canonical bytes, seats,
/// catalogs, or preassembled transactions.
pub struct SemaBootstrapAuthority {
    assembler: SemaTransactionAssembler,
    grammar: BootstrapGrammarIdentities,
    seed: SeedAuthorityState,
    used_names: BTreeSet<EncodedName>,
    used_canonical_bytes: BTreeSet<Vec<u8>>,
    staged: BTreeMap<SourceRequest, StagedBootstrapChange>,
    replays: BTreeMap<SourceRequest, AuthorizedBootstrap>,
}

impl SemaBootstrapAuthority {
    /// Start a fresh, empty authority world from the identity-free core seed.
    ///
    /// This constructor accepts no identity, proof, receipt, seat, catalog, or
    /// canonical bytes. Durable/restart replay and atomic installation belong
    /// to hqu.30 rather than this in-memory authority world.
    pub fn new() -> Result<Self, BootstrapAssemblyError> {
        let mut used_names = BTreeSet::new();
        let mut used_canonical_bytes = BTreeSet::new();
        let assembler = SemaTransactionAssembler::new();
        let grammar =
            assembler.bootstrap_grammar(mint_name(&mut used_names)?, mint_name(&mut used_names)?);
        let seed = SeedAuthorityState::mint(&mut used_names, &mut used_canonical_bytes)?;
        Ok(Self {
            assembler,
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
        let catalog = self.seed.catalog_for(&self.assembler, &request.placement)?;
        let planning_reader = self.assembler.bootstrap_reader(
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
        let reader =
            self.assembler
                .bootstrap_reader(self.grammar.clone(), catalog.clone(), authority)?;
        let sealed_plan = reader.plan(&request.source)?;
        let assignments = allocation.assignments_for(&sealed_plan, &self.assembler)?;
        let transaction = reader.seal(
            &sealed_plan,
            &assignments,
            &TextualMetadataTransition::new(catalog.metadata().clone(), allocation.after.clone()),
            &proof,
        )?;
        let canonical_source = reader.write(&transaction)?;
        let true_names = transaction.strict_value_true_names()?;
        let result = AuthorizedBootstrap {
            canonical_source,
            name_view: AuthorityNameView::from_snapshot(&allocation.after, &request.placement),
            reader,
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

/// An opaque, authority-sealed result.  Its receipt and transaction remain
/// private to Sema until the atomic persistence owner consumes the stage.
#[derive(Clone)]
pub struct AuthorizedBootstrap {
    canonical_source: String,
    name_view: AuthorityNameView,
    reader: BootstrapReader<SemaNamingAuthority>,
    transaction: PreparedBootstrapTransaction<SemaNamingAuthority>,
}

impl AuthorizedBootstrap {
    /// Canonical source projection after receipt validation.
    pub fn canonical_source(&self) -> &str {
        &self.canonical_source
    }

    /// Sealed textual projection for identities already issued in this result.
    pub fn name_view(&self) -> &AuthorityNameView {
        &self.name_view
    }

    /// Reader that validates this authority-branded prepared transaction.
    pub fn reader(&self) -> &BootstrapReader<SemaNamingAuthority> {
        &self.reader
    }

    /// Prepared transaction created and sealed by Sema authority only.
    pub fn transaction(&self) -> &PreparedBootstrapTransaction<SemaNamingAuthority> {
        &self.transaction
    }
}

/// Read-only `EncodedName` to textual-metadata projection carried by a sealed
/// authority result. Its records are created exclusively from the authority's
/// canonical textual metadata; callers cannot add or replace associations.
#[derive(Clone)]
pub struct AuthorityNameView {
    metadata: BTreeMap<EncodedName, TextualMetadata>,
}

impl AuthorityNameView {
    fn from_snapshot(snapshot: &TextualMetadataSnapshot, placement: &SourcePlacement) -> Self {
        let metadata = snapshot
            .records()
            .iter()
            .map(|record| {
                let file_path = if record.address.module_path == ["builtin".to_owned()] {
                    vec!["builtin".to_owned()]
                } else {
                    placement.file_path().to_vec()
                };
                (
                    record.encoded_name,
                    TextualMetadata::new(
                        record.address.textual_name.clone(),
                        ModulePlacement::new(record.address.module_path.clone()),
                        FilePlacement::new(file_path),
                    ),
                )
            })
            .collect();
        Self { metadata }
    }
}

impl NameView for AuthorityNameView {
    fn association(&self, _encoded_name: &EncodedName) -> Option<&NameAssociation> {
        None
    }

    fn textual_metadata(&self, encoded_name: &EncodedName) -> Option<&TextualMetadata> {
        self.metadata.get(encoded_name)
    }
}

impl EncodedNameResolver for AuthorityNameView {
    fn resolve(&self, encoded_name: &EncodedName) -> Option<&TextualName> {
        self.metadata
            .get(encoded_name)
            .map(TextualMetadata::textual_name)
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
        assembler: &SemaTransactionAssembler,
        placement: &SourcePlacement,
    ) -> Result<BootstrapCatalog, BootstrapAssemblyError> {
        assembler
            .bootstrap_catalog(
                placement.module_path().to_vec(),
                BootstrapCatalogMetadata::new(self.metadata.clone(), self.schemas.clone()),
                self.priors.clone(),
                BootstrapVersionPolicy::exact(EthosVersion::new(1, 0, 0)),
                self.canonical_order.clone(),
            )
            .map_err(BootstrapAssemblyError::from)
    }
}

#[derive(Clone)]
pub struct SemaNamingAuthority {
    before: TextualMetadataSnapshot,
    after: Option<TextualMetadataSnapshot>,
    new_canonical_bytes: BTreeMap<EncodedName, Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorityProof {
    private: (),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorityReceipt(PreparedBootstrapDraft);

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
        AuthorityProof { private: () }
    }

    fn approves(&self, draft: &PreparedBootstrapDraft) -> bool {
        self.after.as_ref().is_some_and(|after| {
            draft.naming_transition.before() == &self.before
                && draft.naming_transition.after() == after
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
        assembler: &SemaTransactionAssembler,
    ) -> Result<NamingAssignments, BootstrapAssemblyError> {
        if self.plan.len() != plan.declarations().len() {
            return Err(BootstrapAssemblyError::PlanChangedDuringAuthorityAssembly);
        }
        assembler
            .naming_assignments(
                self.plan
                    .iter()
                    .zip(plan.declarations())
                    .map(|((planned, identity, disposition), declaration)| {
                        if planned.ordinal() != declaration.occurrence().ordinal() {
                            return Err(BootstrapAssemblyError::PlanChangedDuringAuthorityAssembly);
                        }
                        Ok(assembler.naming_assignment(
                            declaration.occurrence(),
                            *identity,
                            disposition.clone(),
                        ))
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
}
