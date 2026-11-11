//! Which script types the process holds a value of (#1746). Each value of
//! a script type holds its type's [`Token`], and every generation of the
//! process gives the values it makes the same token, so a save that
//! removes a type or changes its fields asks one count whether anything
//! still holds a value of it: the program's state, a value native code
//! holds as a handle, a pending task or a captured closure. The
//! count is kept where values are made and dropped, so a save walks no
//! value.

use crate::type_def;
use std::collections::HashMap;
use std::sync::Arc;

/// What a value of one script type holds while it lives: a reference its
/// type's count counts.
#[derive(Clone)]
pub(crate) struct Token(Arc<()>);

/// A script type's token as a model keeps it to give each value it makes.
/// A model holds this, never the token itself, so only values count.
#[derive(Clone)]
pub(crate) struct Entry(Arc<Token>);

type Entries = HashMap<type_def::Identity, Entry>;

/// Each script type's entry, by its identity, for the process's life: a
/// type keeps its token whatever generation makes its values.
#[cfg(feature = "sync")]
static ENTRIES: std::sync::LazyLock<std::sync::Mutex<Entries>> =
    std::sync::LazyLock::new(std::sync::Mutex::default);

// A build without `sync` runs its program on one thread.
#[cfg(not(feature = "sync"))]
thread_local! {
    static ENTRIES: std::cell::RefCell<Entries> = std::cell::RefCell::default();
}

fn with_entries<R>(work: impl FnOnce(&mut Entries) -> R) -> R {
    #[cfg(feature = "sync")]
    return work(
        &mut ENTRIES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    #[cfg(not(feature = "sync"))]
    return ENTRIES.with_borrow_mut(work);
}

impl Entry {
    /// The entry of the script type `identity`, which every model of the
    /// process shares.
    pub(crate) fn of(identity: &type_def::Identity) -> Self {
        with_entries(|entries| {
            entries
                .entry(identity.clone())
                .or_insert_with(|| Self(Arc::new(Token(Arc::new(())))))
                .clone()
        })
    }

    /// The token a new value of this type holds.
    pub(crate) fn token(&self) -> Token {
        (*self.0).clone()
    }
}

/// Whether a value of the script type `identity` lives in this process.
#[must_use]
pub(crate) fn holds(identity: &type_def::Identity) -> bool {
    with_entries(|entries| {
        entries
            .get(identity)
            // The entry's own token is the one reference that is no value's.
            .is_some_and(|entry| Arc::strong_count(&(entry.0).0) > 1)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_type_is_held_while_a_value_holds_its_token() {
        let identity = type_def::Identity::Script(type_def::Key::new(
            "census".into(),
            Box::default(),
            "Held".into(),
        ));
        assert!(!holds(&identity));
        let entry = Entry::of(&identity);
        let again = Entry::of(&identity);
        assert!(!holds(&identity));
        let value = entry.token();
        let copy = value.clone();
        assert!(holds(&identity));
        drop(value);
        assert!(holds(&identity));
        drop(copy);
        assert!(!holds(&identity));
        drop(again.token());
        assert!(!holds(&identity));
    }
}
