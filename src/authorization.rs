use std::collections::BTreeSet;

use signal_sema_translator::{AuthorityCapability, AuthorityRole, AuthorizationClaim, PrincipalId};

/// Derive the stable contract principal authenticated by one Unix peer UID.
///
/// The UID comes from the kernel-owned peer credential on the accepted
/// socket, never from request bytes. Domain separation keeps these local
/// operating-system principals distinct from principals minted by any other
/// authentication mechanism.
pub fn principal_for_unix_uid(uid: u32) -> PrincipalId {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sema-translator/unix-peer-uid/v1");
    hasher.update(&uid.to_le_bytes());
    PrincipalId::new(*hasher.finalize().as_bytes())
}

/// Injected authority policy binding an authenticated connection identity to
/// one typed request claim.
///
/// The production Unix listener derives the authenticated [`PrincipalId`]
/// from the kernel-owned peer UID. This policy checks that the carried role
/// and capability are granted to that identity.
pub trait AuthorizationPolicy: Send + Sync + 'static {
    fn allows(
        &self,
        authenticated: PrincipalId,
        claim: &AuthorizationClaim,
        required_role: AuthorityRole,
        required_capability: AuthorityCapability,
    ) -> bool;
}

/// Explicit in-memory grants used by process configuration and tests.
#[derive(Clone, Default)]
pub struct StaticAuthorizationPolicy {
    grants: BTreeSet<(PrincipalId, AuthorityRole, AuthorityCapability)>,
}

impl StaticAuthorizationPolicy {
    /// Create an empty deny-by-default policy.
    pub fn new() -> Self {
        Self::default()
    }

    /// Grant one exact role/capability pair to one typed principal.
    pub fn grant(
        mut self,
        principal: PrincipalId,
        role: AuthorityRole,
        capability: AuthorityCapability,
    ) -> Self {
        self.grants.insert((principal, role, capability));
        self
    }

    /// Grant every revision-1 authority capability to one configured
    /// principal.
    pub fn grant_all(mut self, principal: PrincipalId) -> Self {
        for (role, capability) in [
            (AuthorityRole::Reader, AuthorityCapability::Read),
            (
                AuthorityRole::UniversalAuthor,
                AuthorityCapability::SealUniversal,
            ),
            (
                AuthorityRole::UniversalMaintainer,
                AuthorityCapability::Rename,
            ),
            (
                AuthorityRole::RustVocabularyPublisher,
                AuthorityCapability::PublishRustVocabulary,
            ),
        ] {
            self.grants.insert((principal, role, capability));
        }
        self
    }
}

impl AuthorizationPolicy for StaticAuthorizationPolicy {
    fn allows(
        &self,
        authenticated: PrincipalId,
        claim: &AuthorizationClaim,
        required_role: AuthorityRole,
        required_capability: AuthorityCapability,
    ) -> bool {
        claim.principal == authenticated
            && claim.role == required_role
            && claim.capability == required_capability
            && self
                .grants
                .contains(&(authenticated, required_role, required_capability))
    }
}
