//! Registered Rust enum metadata used by `switch` patterns.

use crate::func::{SendSync, Shared};
use crate::types::Variant;
use crate::{Dynamic, Identifier};
use std::any::TypeId;
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

/// A host callback that projects an opaque enum value into its variant index
/// and positional payload.
#[cfg(not(feature = "sync"))]
type EnumExtractor = dyn Fn(&Dynamic) -> Option<(usize, Vec<Dynamic>)>;

/// A host callback that projects an opaque enum value into its variant index
/// and positional payload.
#[cfg(feature = "sync")]
type EnumExtractor = dyn Fn(&Dynamic) -> Option<(usize, Vec<Dynamic>)> + Send + Sync;

/// Metadata for one registered Rust enum variant.
#[derive(Debug, Clone, Hash)]
pub(crate) struct VariantInfo {
    pub name: Identifier,
    pub arity: usize,
}

/// Metadata and extraction behavior for one registered Rust enum type.
#[derive(Clone)]
pub(crate) struct EnumRegistration {
    type_id: TypeId,
    variants: Vec<VariantInfo>,
    extractor: Shared<EnumExtractor>,
}

impl EnumRegistration {
    /// Build a registration for `T`.
    pub fn new<T>(
        variants: &[(&str, usize)],
        extractor: impl Fn(&T) -> (usize, Vec<Dynamic>) + SendSync + 'static,
    ) -> Self
    where
        T: Variant + Clone,
    {
        let variants = variants
            .iter()
            .map(|(name, arity)| VariantInfo {
                name: (*name).into(),
                arity: *arity,
            })
            .collect();

        let extractor = move |value: &Dynamic| {
            let value = value.read_lock::<T>()?;
            Some(extractor(&value))
        };

        Self {
            type_id: TypeId::of::<T>(),
            variants,
            extractor: Shared::new(extractor),
        }
    }

    /// Rust [`TypeId`] of the registered enum.
    #[inline(always)]
    #[must_use]
    pub const fn type_id(&self) -> TypeId {
        self.type_id
    }

    /// Look up a variant by its script-visible name.
    #[inline]
    #[must_use]
    pub fn find_variant(&self, name: &str) -> Option<(usize, &VariantInfo)> {
        self.variants
            .iter()
            .enumerate()
            .find(|(_, variant)| variant.name == name)
    }

    /// Project a value of this enum type into its variant index and payload.
    #[inline(always)]
    #[must_use]
    pub fn extract(&self, value: &Dynamic) -> Option<(usize, Vec<Dynamic>)> {
        (self.extractor)(value)
    }

    /// Get metadata for a variant index.
    #[inline(always)]
    #[must_use]
    pub fn variant(&self, index: usize) -> Option<&VariantInfo> {
        self.variants.get(index)
    }
}
