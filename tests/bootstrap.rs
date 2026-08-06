use std::collections::BTreeMap;

use crate::bootstrap::{
    AuthorizedBootstrapTransition, AuthorizedStreamSeats, BootstrapAssemblyError,
    BootstrapAuthorityIdentity, BootstrapAuthorityRevision, BootstrapTransactionAssembler,
};
use core_ethos::bootstrap::{
    BootstrapCatalog, BootstrapGrammarIdentities, BootstrapPriorIdentities,
    BootstrapPriorVocabulary, BootstrapReadError, BootstrapVersionPolicy, CanonicalIdentityOrder,
    EthosVersion, IdentityDisposition, IdentitySchema, IdentitySchemaCatalog, InterfaceRole,
    NomosSchema, SchemaRole, TextualMetadataRecord, TextualMetadataSnapshot,
    TextualProjectionAddress,
};
use name_table::{LocalEncodedId, Name};
use signal_sema_translator::{VocabularyEncodedId, VocabularyRoot};
use structural_codec::EncodedNameResolver;

const SOURCE: &str = "Interface.{1 0 0}\n[]\n{\n  []\n  []\n  []\n  [Domain.[All Health.HealthDomain] HealthDomain.[Body]]\n}\n";
const STREAM_SOURCE: &str = "Interface.{1 0 0}\n[]\n{[] [] [] [Flow.Stream.(String Integer)]}";

fn id(local: u16) -> VocabularyEncodedId {
    VocabularyEncodedId::new(VocabularyRoot::Universal, vec![LocalEncodedId::new(local)])
        .expect("test authority identity is nonempty")
}

fn record(
    module: &[&str],
    owner: Option<VocabularyEncodedId>,
    spelling: &str,
    identity: VocabularyEncodedId,
) -> TextualMetadataRecord {
    TextualMetadataRecord {
        address: TextualProjectionAddress {
            module_path: module.iter().map(|part| (*part).to_owned()).collect(),
            lexical_owner: owner,
            visible_name: spelling.to_owned(),
        },
        encoded_name: identity,
    }
}

fn prior_identities() -> BootstrapPriorIdentities {
    BootstrapPriorIdentities {
        interface_kind: id(1),
        nexus_kind: id(2),
        sema_kind: id(3),
        input_role: id(4),
        output_role: id(5),
        refusal_role: id(6),
        string_type: id(7),
        integer_type: id(8),
        boolean_type: id(9),
        unit_type: id(10),
        vector_shape: id(11),
        option_shape: id(12),
        map_shape: id(13),
        result_shape: id(14),
        stream_nomos: id(15),
        stream_shape: id(15),
        stream_identity_shape: id(16),
    }
}

fn base_catalog() -> BootstrapCatalog {
    let specifications = [
        (
            1,
            "Interface",
            vec![SchemaRole::FileKind(
                core_ethos::bootstrap::EthosKind::Interface,
            )],
        ),
        (
            2,
            "Nexus",
            vec![SchemaRole::FileKind(
                core_ethos::bootstrap::EthosKind::Nexus,
            )],
        ),
        (
            3,
            "Sema",
            vec![SchemaRole::FileKind(core_ethos::bootstrap::EthosKind::Sema)],
        ),
        (
            4,
            "Input",
            vec![SchemaRole::InterfaceRole(InterfaceRole::Input)],
        ),
        (
            5,
            "Output",
            vec![SchemaRole::InterfaceRole(InterfaceRole::Output)],
        ),
        (
            6,
            "Refusal",
            vec![SchemaRole::InterfaceRole(InterfaceRole::Refusal)],
        ),
        (7, "String", vec![SchemaRole::Nominal { persistent: true }]),
        (8, "Integer", vec![SchemaRole::Nominal { persistent: true }]),
        (9, "Boolean", vec![SchemaRole::Nominal { persistent: true }]),
        (10, "Unit", vec![SchemaRole::Nominal { persistent: true }]),
        (11, "Vector", vec![SchemaRole::Shape { arity: 1 }]),
        (12, "Option", vec![SchemaRole::Shape { arity: 1 }]),
        (13, "Map", vec![SchemaRole::Shape { arity: 2 }]),
        (14, "Result", vec![SchemaRole::Shape { arity: 2 }]),
        (
            15,
            "Stream",
            vec![
                SchemaRole::Shape { arity: 1 },
                SchemaRole::Nomos(NomosSchema::StreamInitiation { arity: 2 }),
            ],
        ),
        (16, "StreamIdentity", vec![SchemaRole::Shape { arity: 1 }]),
    ];
    let metadata = TextualMetadataSnapshot::new(
        specifications
            .iter()
            .map(|(local, spelling, _)| record(&["builtin"], None, spelling, id(*local)))
            .collect(),
    )
    .expect("valid prior metadata");
    let schemas = IdentitySchemaCatalog::new(
        specifications
            .iter()
            .map(|(local, _, roles)| IdentitySchema::new(id(*local), roles.clone()).unwrap())
            .collect(),
    )
    .expect("valid prior schemas");
    let priors = BootstrapPriorVocabulary::new(prior_identities(), &schemas, &metadata)
        .expect("valid prior relationships");
    let order = CanonicalIdentityOrder::new(
        specifications
            .iter()
            .map(|(local, _, _)| (id(*local), vec![0x10, *local as u8])),
    )
    .expect("unique prior order");
    BootstrapCatalog::new(
        vec!["app".to_owned()],
        metadata,
        schemas,
        priors,
        BootstrapVersionPolicy::exact(EthosVersion::new(1, 0, 0)),
        order,
    )
    .expect("valid bootstrap catalog")
}

fn assembler(catalog: BootstrapCatalog, authority_byte: u8) -> BootstrapTransactionAssembler {
    BootstrapTransactionAssembler::new(
        BootstrapAuthorityIdentity::new([authority_byte; 32]),
        BootstrapAuthorityRevision::new(7),
        BootstrapGrammarIdentities {
            document: id(900),
            syntax: id(901),
        },
        catalog,
    )
}

fn domain_after(before: &TextualMetadataSnapshot) -> TextualMetadataSnapshot {
    let domain = id(100);
    let health_domain = id(103);
    let mut records = before.records().to_vec();
    records.extend([
        record(&["app"], None, "Domain", domain.clone()),
        record(&["app"], Some(domain.clone()), "All", id(101)),
        record(&["app"], Some(domain), "Health", id(102)),
        record(&["app"], None, "HealthDomain", health_domain.clone()),
        record(&["app"], Some(health_domain), "Body", id(104)),
    ]);
    TextualMetadataSnapshot::new(records).expect("one exact authority projection per identity")
}

fn domain_approval(before: &TextualMetadataSnapshot) -> AuthorizedBootstrapTransition {
    AuthorizedBootstrapTransition::new(
        domain_after(before),
        (100..=104)
            .map(|local| (id(local), vec![0x80, local as u8]))
            .collect(),
        BTreeMap::new(),
    )
}

#[test]
fn production_boundary_seals_exact_approval_and_releases_resolver_after_validation() {
    let catalog = base_catalog();
    let approval = domain_approval(catalog.metadata());
    let assembly = assembler(catalog, 0x51)
        .assemble(SOURCE, approval)
        .expect("seal authority-approved source");

    assembly
        .reader()
        .validate_transaction(assembly.transaction())
        .expect("receipt remains valid for its exact authority configuration");
    assert_eq!(
        assembly.canonical_source(),
        assembly
            .reader()
            .write(assembly.transaction())
            .expect("canonical writer revalidates")
    );
    assert_eq!(
        assembly.resolver().resolve(&id(100)),
        Some(&Name::new("Domain"))
    );
    assert!(
        assembly
            .transaction()
            .identity_dispositions()
            .values()
            .all(|disposition| matches!(disposition, IdentityDisposition::New { .. }))
    );
}

#[test]
fn restart_reuses_authorized_identities_without_reminting() {
    let initial_catalog = base_catalog();
    let initial = assembler(initial_catalog.clone(), 0x52)
        .assemble(SOURCE, domain_approval(initial_catalog.metadata()))
        .expect("initial authority transaction");
    let schemas = IdentitySchemaCatalog::new(
        initial_catalog
            .schemas()
            .entries()
            .chain(initial.transaction().schema_additions().entries())
            .cloned()
            .collect(),
    )
    .expect("restart schema catalog");
    let metadata = initial.transaction().naming_transition().after().clone();
    let priors = BootstrapPriorVocabulary::new(prior_identities(), &schemas, &metadata)
        .expect("restart priors");
    let restarted_catalog = BootstrapCatalog::new(
        vec!["app".to_owned()],
        metadata.clone(),
        schemas,
        priors,
        BootstrapVersionPolicy::exact(EthosVersion::new(1, 0, 0)),
        initial.transaction().canonical_order().clone(),
    )
    .expect("restart catalog");
    let restarted = assembler(restarted_catalog, 0x52)
        .assemble(
            SOURCE,
            AuthorizedBootstrapTransition::new(metadata, BTreeMap::new(), BTreeMap::new()),
        )
        .expect("existing identities are explicitly preserved");

    assert!(
        restarted
            .transaction()
            .identity_dispositions()
            .values()
            .all(|disposition| disposition == &IdentityDisposition::Existing)
    );
    assert_eq!(
        restarted.resolver().resolve(&id(100)),
        Some(&Name::new("Domain"))
    );
}

#[test]
fn authority_bytes_and_generated_stream_seats_are_exact_not_inferred() {
    let catalog = base_catalog();
    let mut records = catalog.metadata().records().to_vec();
    records.extend([
        record(&["app"], None, "Flow", id(200)),
        record(&["app"], None, "StartFlow", id(201)),
        record(&["app"], None, "StopFlow", id(202)),
    ]);
    let after = TextualMetadataSnapshot::new(records).expect("Stream authority metadata");
    let approval = AuthorizedBootstrapTransition::new(
        after,
        [
            (id(200), vec![0x90, 0]),
            (id(201), vec![0x90, 1]),
            (id(202), vec![0x90, 2]),
        ]
        .into_iter()
        .collect(),
        [(id(200), AuthorizedStreamSeats::new(id(201), id(202)))]
            .into_iter()
            .collect(),
    );
    let assembly = assembler(catalog, 0x53)
        .assemble(STREAM_SOURCE, approval)
        .expect("explicit Stream seats seal");
    let [generated] = assembly.transaction().generated_streams() else {
        panic!("one generated Stream transaction expected")
    };
    assert_eq!(generated.output.name, id(200));
    assert_eq!(generated.initiation.name, id(201));
    assert_eq!(generated.termination.name, id(202));
    assert!(!assembly.canonical_source().contains("StartFlow"));
    assert!(!assembly.canonical_source().contains("StopFlow"));
}

#[test]
fn missing_and_unused_authority_seats_are_typed_refusals() {
    let catalog = base_catalog();
    let after = domain_after(catalog.metadata());
    let missing = AuthorizedBootstrapTransition::new(
        after.clone(),
        (101..=104)
            .map(|local| (id(local), vec![0x80, local as u8]))
            .collect(),
        BTreeMap::new(),
    );
    assert!(matches!(
        assembler(catalog.clone(), 0x54).assemble(SOURCE, missing),
        Err(BootstrapAssemblyError::MissingAuthorizedCanonicalBytes { identity })
            if identity == id(100)
    ));

    let mut bytes = (100..=104)
        .map(|local| (id(local), vec![0x80, local as u8]))
        .collect::<BTreeMap<_, _>>();
    bytes.insert(id(300), vec![0x99]);
    let unused = AuthorizedBootstrapTransition::new(after, bytes, BTreeMap::new());
    assert!(matches!(
        assembler(catalog, 0x54).assemble(SOURCE, unused),
        Err(BootstrapAssemblyError::UnusedAuthorizedCanonicalBytes { identity })
            if identity == id(300)
    ));
}

#[test]
fn receipt_is_configuration_bound_and_obsolete_six_slot_source_is_rejected() {
    let catalog = base_catalog();
    let first = assembler(catalog.clone(), 0x55)
        .assemble(SOURCE, domain_approval(catalog.metadata()))
        .expect("first authority");
    let other = assembler(catalog.clone(), 0x56)
        .assemble(SOURCE, domain_approval(catalog.metadata()))
        .expect("other authority");
    assert!(matches!(
        other.reader().validate_transaction(first.transaction()),
        Err(BootstrapReadError::NamingAuthorityReceiptRejected)
    ));

    assert!(matches!(
        assembler(catalog.clone(), 0x55).assemble(
            "{}\n[]\n[]\n{}\n{}\n{}",
            domain_approval(catalog.metadata())
        ),
        Err(BootstrapAssemblyError::Read(_))
    ));
}
