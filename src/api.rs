//! Module defining the public API of the Rawr engine.

pub mod eval;

pub mod run;

pub mod compile;

pub mod json;

pub mod files;

pub mod register;

pub mod call_fn;

pub mod script;

pub mod options;

pub mod optimize;

pub mod limits;

pub mod events;

pub mod formatting;

pub mod build_type;

pub mod deprecated;

use crate::func::{waited_read, waited_write, Locked};
use crate::types::StringsInterner;
use crate::{Dynamic, Engine};

/// Default limits.
pub mod default_limits {
    #[cfg(feature = "internals")]
    pub use super::limits::default_limits::*;

    /// Maximum number of parameters in functions with [`Dynamic`][crate::Dynamic] support.
    pub const MAX_DYNAMIC_PARAMETERS: usize = 16;
    /// Maximum number of strings interned.
    pub const MAX_STRINGS_INTERNED: usize = 256;
}

impl Engine {
    /// Set the maximum number of strings to be interned.
    #[inline(always)]
    pub fn set_max_strings_interned(&mut self, max: usize) -> &mut Self {
        if max == 0 {
            self.interned_strings = None;
        } else if let Some(ref interner) = self.interned_strings {
            waited_write(interner).set_max(max);
        } else {
            self.interned_strings = Some(Locked::new(StringsInterner::new(max)));
        }
        self
    }
    /// The maximum number of strings to be interned.
    #[inline(always)]
    #[must_use]
    pub fn max_strings_interned(&self) -> usize {
        self.interned_strings
            .as_ref()
            .map_or(0, |interner| waited_read(interner).max())
    }

    /// Get the default value of the custom state for each evaluation run.
    #[inline(always)]
    pub const fn default_tag(&self) -> &Dynamic {
        &self.def_tag
    }
    /// Get a mutable reference to the default value of the custom state for each evaluation run.
    #[inline(always)]
    pub fn default_tag_mut(&mut self) -> &mut Dynamic {
        &mut self.def_tag
    }
    /// Set the default value of the custom state for each evaluation run.
    #[inline(always)]
    pub fn set_default_tag(&mut self, value: impl Into<Dynamic>) -> &mut Self {
        self.def_tag = value.into();
        self
    }
}
