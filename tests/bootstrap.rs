use crate::bootstrap::{BootstrapAssemblyError, SemaBootstrapAuthority, SourcePlacement};
use core_ethos::bootstrap::BootstrapReadError;

const SOURCE: &str = "Interface.{1 0 0}\n[]\n{\n  []\n  []\n  []\n  [Domain.[All Health.HealthDomain] HealthDomain.[Body]]\n}\n";
const STREAM_SOURCE: &str = "Interface.{1 0 0}\n[]\n{[] [] [] [Flow.Stream.(String Integer)]}";

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
fn bundled_stream_is_refused_before_any_authority_stage_is_created() {
    let mut authority = SemaBootstrapAuthority::new().expect("authority owns its seed allocation");
    assert!(matches!(
        authority.authorize(STREAM_SOURCE, placement()),
        Err(BootstrapAssemblyError::Read(
            BootstrapReadError::BundledStreamUnsupported
        ))
    ));
    authority
        .authorize(SOURCE, placement())
        .expect("the earlier Stream refusal did not consume a staged authority change");
}

#[test]
fn distinct_input_cannot_overtake_a_staged_change() {
    let mut authority = SemaBootstrapAuthority::new().expect("authority owns its seed allocation");
    authority
        .authorize(SOURCE, placement())
        .expect("initial stage");
    assert!(matches!(
        authority.authorize("Interface.{1 0 0}\n[]\n{[] [] [] []}", placement()),
        Err(BootstrapAssemblyError::PendingStagedChange)
    ));
}
