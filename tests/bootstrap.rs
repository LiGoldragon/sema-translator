use crate::bootstrap::{SemaBootstrapAuthority, SourcePlacement};

const SOURCE: &str = "Interface.{1 0 0}\n[]\n{\n  []\n  []\n  []\n  [Domain.[All Health.HealthDomain] HealthDomain.[Body]]\n}\n";

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
        .authorize("Interface.{1 0 0}\n[]\n{[] [] [] []}", placement())
        .expect("a distinct source gets its own private stage");
}

#[test]
fn admitted_domain_shape_resolves_in_type_applications() {
    let mut authority = SemaBootstrapAuthority::new().expect("authority owns its seed allocation");
    authority
        .admit_domain_shape("ScopeOf", 1)
        .expect("authority admits a domain shape before authorization");
    let source = "Interface.{1 0 0}\n[]\n{[] [] [] [DomainScope.ScopeOf<Domain> Domain.[All]]}";
    let result = authority
        .authorize(source, placement())
        .expect("authorized source with admitted domain shape constructor");
    assert!(result.canonical_source().contains("DomainScope"));
    assert!(result.canonical_source().contains("ScopeOf"));
}

#[test]
fn domain_shape_without_admission_is_an_unresolved_reference() {
    let mut authority = SemaBootstrapAuthority::new().expect("authority owns its seed allocation");
    let source = "Interface.{1 0 0}\n[]\n{[] [] [] [DomainScope.ScopeOf<Domain> Domain.[All]]}";
    assert!(
        authority.authorize(source, placement()).is_err(),
        "ScopeOf must be admitted before authorization to resolve as a shape"
    );
}
