//! Sealed support-only operation endpoint proofs.

use crate::{host, Dynamic, ImmutableString};

pub(crate) type Invoke = fn(ImmutableString) -> Dynamic;

/// One facade-owned support-only operation endpoint.
///
/// This is neither an item nor a type declaration. Its callable is available
/// only after the compiler has joined the exact imported operation site to the
/// profile proof that sealed this contract.
#[derive(Clone, Debug)]
pub(crate) struct Contract {
    path: host::path::Path,
    target: host::path::Path,
    site: [u8; 32],
    profile: [u8; 32],
    source: [u8; 32],
    projection: [u8; 32],
    linked: [u8; 32],
    representation: host::type_def::Identity,
    invoke: Invoke,
}

impl Contract {
    #[allow(
        clippy::too_many_arguments,
        reason = "the endpoint proof bundle is intentionally indivisible"
    )]
    #[cfg(feature = "rust-import")]
    pub(crate) fn new(
        path: host::path::Path,
        target: host::path::Path,
        site: [u8; 32],
        profile: [u8; 32],
        source: [u8; 32],
        projection: [u8; 32],
        linked: [u8; 32],
        representation: std::any::TypeId,
        invoke: Invoke,
    ) -> Result<Self, host::Error> {
        if [site, profile, source, projection, linked]
            .iter()
            .any(|value| value == &[0; 32])
        {
            return Err(host::Error::ConflictingHostIdentity {
                existing: path.to_string().into(),
                incoming: path.to_string().into(),
            });
        }
        Ok(Self {
            path,
            target,
            site,
            profile,
            source,
            projection,
            linked,
            representation: host::type_def::Identity::linked(representation),
            invoke,
        })
    }

    pub(crate) const fn path_ref(&self) -> &host::path::Path {
        &self.path
    }

    pub(crate) fn path(&self) -> String {
        self.path.to_string()
    }

    pub(crate) const fn target(&self) -> &host::path::Path {
        &self.target
    }

    pub(crate) const fn site(&self) -> &[u8; 32] {
        &self.site
    }

    pub(crate) const fn profile(&self) -> &[u8; 32] {
        &self.profile
    }

    pub(crate) const fn source(&self) -> &[u8; 32] {
        &self.source
    }

    pub(crate) const fn projection(&self) -> &[u8; 32] {
        &self.projection
    }

    pub(crate) const fn linked(&self) -> &[u8; 32] {
        &self.linked
    }

    pub(crate) fn invoke(&self, input: ImmutableString) -> Dynamic {
        (self.invoke)(input)
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn normalize_profile(
        &mut self,
        profile: &host::facade::ProfileFinalization<'_>,
    ) -> usize {
        usize::from(profile.normalize_identity(&self.target, &mut self.representation))
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn retained_bytes(&self) -> Option<usize> {
        self.path
            .retained_bytes()?
            .checked_add(self.target.retained_bytes()?)?
            .checked_add(160)
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn try_clone(&self) -> Result<Self, rawr_import::surface::Error> {
        Ok(Self {
            path: self.path.try_clone()?,
            target: self.target.try_clone()?,
            site: self.site,
            profile: self.profile,
            source: self.source,
            projection: self.projection,
            linked: self.linked,
            representation: self.representation.clone(),
            invoke: self.invoke,
        })
    }
}

impl PartialEq for Contract {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
            && self.target == other.target
            && self.site == other.site
            && self.profile == other.profile
            && self.source == other.source
            && self.projection == other.projection
            && self.linked == other.linked
            && self.representation == other.representation
    }
}

impl Eq for Contract {}

impl PartialOrd for Contract {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Contract {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (
            &self.path,
            &self.target,
            self.site,
            self.profile,
            self.source,
            self.projection,
            self.linked,
            &self.representation,
        )
            .cmp(&(
                &other.path,
                &other.target,
                other.site,
                other.profile,
                other.source,
                other.projection,
                other.linked,
                &other.representation,
            ))
    }
}

impl std::hash::Hash for Contract {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.path.hash(state);
        self.target.hash(state);
        self.site.hash(state);
        self.profile.hash(state);
        self.source.hash(state);
        self.projection.hash(state);
        self.linked.hash(state);
        self.representation.hash(state);
    }
}

impl crate::preview::digest::Write for Contract {
    fn write(
        &self,
        writer: &mut crate::preview::digest::Writer,
    ) -> Result<(), crate::preview::digest::Error> {
        writer.tag(0x2d00);
        writer.text(&self.path.to_string());
        writer.text(&self.target.to_string());
        writer.bytes(&self.site);
        writer.bytes(&self.profile);
        writer.bytes(&self.source);
        writer.bytes(&self.projection);
        writer.bytes(&self.linked);
        Ok(())
    }
}
