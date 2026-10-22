//! Sealed signature-only host type proofs.

use crate::{host, ImmutableString};
use std::any::TypeId;

/// One facade-owned signature-only linked type contract.
///
/// A support contract is not a type declaration. It grants no source name,
/// capability, storage, structure, method, projection, or runtime registry
/// authority. The stable proof fields authenticate the compiler/profile join;
/// the native identity is consulted only after that join to validate the
/// evaluator-private transient representation.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SignatureContract {
    path: host::path::Path,
    role: ImmutableString,
    site: [u8; 32],
    profile: [u8; 32],
    linked: [u8; 32],
    representation: host::type_def::Identity,
}

/// One same-facade reference to a support contract not yet finished.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Local {
    contract: SignatureContract,
}

/// One exact support contract exported by a finished facade.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SupportExport {
    facade_root: host::path::Path,
    facade: host::facade::Fingerprint,
    contract: SignatureContract,
}

impl SignatureContract {
    #[cfg(feature = "rust-import")]
    pub(crate) fn new(
        path: host::path::Path,
        role: impl Into<ImmutableString>,
        site: [u8; 32],
        profile: [u8; 32],
        linked: [u8; 32],
        representation: TypeId,
    ) -> Result<Self, host::Error> {
        let role = role.into();
        if role.is_empty() || site == [0; 32] || profile == [0; 32] || linked == [0; 32] {
            return Err(host::Error::ConflictingHostIdentity {
                existing: path.to_string().into(),
                incoming: path.to_string().into(),
            });
        }
        Ok(Self {
            path,
            role,
            site,
            profile,
            linked,
            representation: host::type_def::Identity::linked(representation),
        })
    }

    /// Return the canonical qualified support path.
    #[must_use]
    pub fn path(&self) -> String {
        self.path.to_string()
    }

    pub(crate) const fn path_ref(&self) -> &host::path::Path {
        &self.path
    }

    /// Return the closed profile role.
    #[must_use]
    pub fn role(&self) -> &str {
        &self.role
    }

    /// Borrow the compiler-authenticated generic support-site selector.
    #[must_use]
    pub const fn site(&self) -> &[u8; 32] {
        &self.site
    }

    /// Borrow the profile-owned specialized support selector.
    #[must_use]
    pub const fn profile(&self) -> &[u8; 32] {
        &self.profile
    }

    /// Borrow the linked-host semantic digest.
    #[must_use]
    pub const fn linked(&self) -> &[u8; 32] {
        &self.linked
    }

    pub(crate) fn representation(&self) -> TypeId {
        self.representation
            .native()
            .expect("installed signature support retains native identity")
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn retained_bytes(&self) -> Option<usize> {
        self.path
            .retained_bytes()?
            .checked_add(self.role.len())
            .and_then(|bytes| bytes.checked_add(96))
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn try_clone(&self) -> Result<Self, rawr_import::surface::Error> {
        Ok(Self {
            path: self.path.try_clone()?,
            role: self.role.clone(),
            site: self.site,
            profile: self.profile,
            linked: self.linked,
            representation: self.representation.clone(),
        })
    }
}

impl Local {
    #[cfg(feature = "rust-import")]
    pub(crate) const fn new(contract: SignatureContract) -> Self {
        Self { contract }
    }

    pub(crate) const fn contract(&self) -> &SignatureContract {
        &self.contract
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn normalize_profile(
        &mut self,
        profile: &host::facade::ProfileFinalization<'_>,
    ) -> usize {
        self.contract.normalize_profile(profile)
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn try_clone(&self) -> Result<Self, rawr_import::surface::Error> {
        Ok(Self::new(self.contract.try_clone()?))
    }
}

impl SupportExport {
    #[cfg(feature = "rust-import")]
    pub(crate) const fn new(
        facade_root: host::path::Path,
        facade: host::facade::Fingerprint,
        contract: SignatureContract,
    ) -> Self {
        Self {
            facade_root,
            facade,
            contract,
        }
    }

    /// Return the exact canonical support path.
    #[must_use]
    pub fn path(&self) -> String {
        self.contract.path()
    }

    /// Borrow the sealed signature-only contract.
    #[must_use]
    pub const fn contract(&self) -> &SignatureContract {
        &self.contract
    }

    pub(crate) const fn facade_root(&self) -> &host::path::Path {
        &self.facade_root
    }

    pub(crate) const fn facade(&self) -> &host::facade::Fingerprint {
        &self.facade
    }

    pub(crate) fn dependency(&self) -> host::facade::Dependency {
        host::facade::Dependency::new(&self.facade_root, self.facade.clone())
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn normalize_profile(
        &mut self,
        profile: &host::facade::ProfileFinalization<'_>,
    ) -> usize {
        profile
            .normalize_fingerprint(&mut self.facade)
            .saturating_add(self.contract.normalize_profile(profile))
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn retained_bytes(&self) -> Option<usize> {
        self.facade_root
            .retained_bytes()?
            .checked_add(self.facade.retained_bytes()?)?
            .checked_add(self.contract.retained_bytes()?)
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn try_clone(&self) -> Result<Self, rawr_import::surface::Error> {
        Ok(Self {
            facade_root: self.facade_root.try_clone()?,
            facade: self.facade.try_clone()?,
            contract: self.contract.try_clone()?,
        })
    }
}

impl SignatureContract {
    #[cfg(feature = "rust-import")]
    pub(crate) fn normalize_profile(
        &mut self,
        profile: &host::facade::ProfileFinalization<'_>,
    ) -> usize {
        usize::from(profile.normalize_identity(&self.path, &mut self.representation))
    }
}

impl crate::preview::digest::Write for SignatureContract {
    fn write(
        &self,
        writer: &mut crate::preview::digest::Writer,
    ) -> Result<(), crate::preview::digest::Error> {
        writer.tag(0x2c00);
        writer.tag(0x2c01);
        writer.text(&self.path.to_string());
        writer.tag(0x2c02);
        writer.text(&self.role);
        writer.tag(0x2c03);
        writer.bytes(&self.site);
        writer.tag(0x2c04);
        writer.bytes(&self.profile);
        writer.tag(0x2c05);
        writer.bytes(&self.linked);
        Ok(())
    }
}
