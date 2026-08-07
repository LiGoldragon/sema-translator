use crate::bootstrap::{SemaBootstrapAuthority, SourcePlacement};

const SOURCE: &str = "Interface.{1 0 0}\n[]\n{\n  []\n  []\n  []\n  []\n  [Domain.[All Health.HealthDomain] HealthDomain.[Body]]\n}\n";

fn placement() -> SourcePlacement {
    SourcePlacement::new(
        vec!["app".to_owned()],
        vec!["app".to_owned(), "domain.ethos".to_owned()],
    )
}

#[test]
fn source_and_placement_are_the_entire_caller_input_and_replay_does_not_remint() {
    let mut authority = SemaBootstrapAuthority::new().expect("authority owns its seed allocation");
    let first = authority
        .authorize(SOURCE, placement())
        .expect("authority mints and seals source-local declarations");
    let replay = authority
        .authorize(SOURCE, placement())
        .expect("the realized result replays without another allocation");

    assert_eq!(first.canonical_source(), replay.canonical_source());
    assert!(first.canonical_source().contains("Domain"));
}

#[test]
fn distinct_sources_receive_private_stages_without_an_atomic_commit() {
    let mut authority = SemaBootstrapAuthority::new().expect("authority owns its seed allocation");
    authority
        .authorize(SOURCE, placement())
        .expect("initial private stage");
    authority
        .authorize("Interface.{1 0 0}\n[]\n{[] [] [] [] []}", placement())
        .expect("a distinct source gets its own private stage");
}

#[test]
fn admitted_domain_shape_resolves_in_type_applications() {
    let mut authority = SemaBootstrapAuthority::new().expect("authority owns its seed allocation");
    authority
        .admit_domain_shape("ScopeOf", 1)
        .expect("authority admits a domain shape before authorization");
    let source = "Interface.{1 0 0}\n[]\n{[] [] [] [] [DomainScope.ScopeOf<Domain> Domain.[All]]}";
    let result = authority
        .authorize(source, placement())
        .expect("authorized source with admitted domain shape constructor");
    assert!(result.canonical_source().contains("DomainScope"));
    assert!(result.canonical_source().contains("ScopeOf"));
}

#[test]
fn domain_shape_without_admission_is_an_unresolved_reference() {
    let mut authority = SemaBootstrapAuthority::new().expect("authority owns its seed allocation");
    let source = "Interface.{1 0 0}\n[]\n{[] [] [] [] [DomainScope.ScopeOf<Domain> Domain.[All]]}";
    assert!(
        authority.authorize(source, placement()).is_err(),
        "ScopeOf must be admitted before authorization to resolve as a shape"
    );
}

fn dependency_placement() -> SourcePlacement {
    SourcePlacement::new(
        vec!["dep".to_owned(), "domain".to_owned()],
        vec![
            "dep".to_owned(),
            "domain".to_owned(),
            "domain.schema".to_owned(),
        ],
    )
}

fn consumer_placement() -> SourcePlacement {
    SourcePlacement::new(
        vec!["consumer".to_owned()],
        vec!["consumer".to_owned(), "consumer.schema".to_owned()],
    )
}

#[test]
fn imports_resolve_against_previously_authorized_sources() {
    let mut authority = SemaBootstrapAuthority::new().expect("authority owns its seed allocation");
    let dependency = "Interface.{1 0 0}\n[]\n{[] [] [] [] [Domain.[All Craft] DomainScopes.Vector<Domain>]}";
    authority
        .authorize(dependency, dependency_placement())
        .expect("authorize the dependency source first");

    let consumer = "Interface.{1 0 0}\n[dep/domain.[Domain DomainScopes]]\n{[] [] [] [] [Domains.Vector<Domain> Match.[Any Full.DomainScopes]]}";
    let result = authority
        .authorize(consumer, consumer_placement())
        .expect("consumer must resolve imports against the authorized dependency");
    assert!(result.canonical_source().contains("Domains"));
    assert!(result.canonical_source().contains("Domain"));
}

#[test]
fn imports_fail_without_prior_authorization_of_the_source() {
    let mut authority = SemaBootstrapAuthority::new().expect("authority owns its seed allocation");
    let consumer = "Interface.{1 0 0}\n[dep/domain.[Domain]]\n{[] [] [] [] [Domains.Vector<Domain>]}";
    assert!(
        authority.authorize(consumer, consumer_placement()).is_err(),
        "imports must fail when the dependency source was never authorized"
    );
}

#[test]
fn imports_resolve_only_via_authorization_not_caller_assertion() {
    let mut authority = SemaBootstrapAuthority::new().expect("authority owns its seed allocation");
    // Admitting a domain shape is not the same as authorizing a source.
    // The shape resolves at ["builtin"], not at the import module path.
    authority
        .admit_domain_shape("Domain", 0)
        .expect("admit a shape named Domain");
    let consumer = "Interface.{1 0 0}\n[dep/domain.[Domain]]\n{[] [] [] [] []}";
    assert!(
        authority.authorize(consumer, consumer_placement()).is_err(),
        "admitted domain shapes do not satisfy import module-path resolution"
    );
}
