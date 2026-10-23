//! A value an imported Rust function returned that still needs its call's
//! output witness.

use super::Witness;
use crate::Dynamic;
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

#[cfg(feature = "sync")]
type Finish = Box<dyn FnOnce(Witness) -> Dynamic + Send + 'static>;
#[cfg(not(feature = "sync"))]
type Finish = Box<dyn FnOnce(Witness) -> Dynamic + 'static>;

/// A host value that becomes its final form once the bridge gives it the
/// exact output witness of the call that returned it.
///
/// A projected iced `Task` is one. The importer's bridge holds the call; the
/// host that builds the value does not.
#[derive(Clone)]
pub struct Unwitnessed(Arc<Mutex<Option<Finish>>>);

impl Unwitnessed {
    /// Wrap the step that builds the final value from its witness.
    #[cfg(feature = "sync")]
    #[must_use]
    pub fn new(finish: impl FnOnce(Witness) -> Dynamic + Send + 'static) -> Self {
        Self(Arc::new(Mutex::new(Some(Box::new(finish)))))
    }

    /// Wrap the step that builds the final value from its witness.
    #[cfg(not(feature = "sync"))]
    #[must_use]
    pub fn new(finish: impl FnOnce(Witness) -> Dynamic + 'static) -> Self {
        Self(Arc::new(Mutex::new(Some(Box::new(finish)))))
    }

    /// Build the final value. `None` when a copy was already finished.
    #[must_use]
    pub fn witness(self, witness: Witness) -> Option<Dynamic> {
        let finish = self.0.lock().unwrap_or_else(PoisonError::into_inner).take()?;
        Some(finish(witness))
    }
}

impl fmt::Debug for Unwitnessed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Unwitnessed(<private>)")
    }
}
