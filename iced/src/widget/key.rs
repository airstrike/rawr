//! Sealed parent-owned reconciliation metadata.

use std::collections::BTreeMap;
use std::fmt;

use iced::advanced::widget::Tree;
use rawr::ImmutableString;

use crate::widget;

/// One exact source key carried by a widget value until its direct parent
/// consumes it.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum Value {
    Integer(i64),
    String(ImmutableString),
}

/// The validated reconciliation mode for one direct child collection.
#[derive(Clone, Default, Eq, PartialEq)]
pub(crate) enum Mode {
    #[default]
    Positional,
    Keyed(Box<[Value]>),
}

/// A candidate child collection does not form one exact keyed set.
#[derive(Clone, Eq, PartialEq, thiserror::Error)]
pub(crate) enum Error {
    #[error("keyed children require a key on direct child {index}")]
    Missing { index: usize },
    #[error("keyed children repeat key {key} at direct children {first} and {second}")]
    Duplicate {
        key: Value,
        first: usize,
        second: usize,
    },
}

impl Value {
    pub(crate) const fn integer(value: i64) -> Self {
        Self::Integer(value)
    }

    pub(crate) fn string(value: ImmutableString) -> Self {
        Self::String(value)
    }
}

impl Mode {
    /// Validate one off-side candidate without mutating retained parent state.
    pub(crate) fn bind(values: &[widget::Value]) -> Result<Self, Error> {
        if values.iter().all(|value| value.key().is_none()) {
            return Ok(Self::Positional);
        }

        let mut seen = BTreeMap::new();
        let mut keys = Vec::with_capacity(values.len());
        for (index, value) in values.iter().enumerate() {
            let Some(key) = value.key().cloned() else {
                return Err(Error::Missing { index });
            };
            if let Some(first) = seen.insert(key.clone(), index) {
                return Err(Error::Duplicate {
                    key,
                    first,
                    second: index,
                });
            }
            keys.push(key);
        }
        Ok(Self::Keyed(keys.into_boxed_slice()))
    }

    /// Relocate retained child trees into candidate order after validation.
    pub(crate) fn reconcile(&self, previous: &Self, trees: &mut Vec<Tree>) {
        let Self::Keyed(keys) = self else {
            return;
        };
        let Self::Keyed(previous) = previous else {
            *trees = keys.iter().map(|_| Tree::empty()).collect();
            return;
        };
        if trees.len() != previous.len() {
            *trees = keys.iter().map(|_| Tree::empty()).collect();
            return;
        }

        let mut retained = previous
            .iter()
            .cloned()
            .zip(std::mem::take(trees))
            .collect::<BTreeMap<_, _>>();
        *trees = keys
            .iter()
            .map(|key| retained.remove(key).unwrap_or_else(Tree::empty))
            .collect();
    }

    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Positional => 0,
            Self::Keyed(keys) => keys.len(),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Integer(value) => value.fmt(formatter),
            Self::String(value) => write!(formatter, "{value:?}"),
        }
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

#[cfg(test)]
mod tests {
    use iced::advanced::widget::{tree, Tree};

    use crate::{runtime, widget};

    use super::{Error, Mode, Value};

    #[test]
    fn collection_validation_requires_all_and_only_unique_keys() {
        let positional = vec![child(None), child(None)];
        assert!(matches!(Mode::bind(&positional), Ok(Mode::Positional)));

        let missing = vec![child(Some(Value::integer(1))), child(None)];
        assert!(matches!(
            Mode::bind(&missing),
            Err(Error::Missing { index: 1 })
        ));

        let duplicate = vec![
            child(Some(Value::string("same".into()))),
            child(Some(Value::string("same".into()))),
        ];
        assert!(matches!(
            Mode::bind(&duplicate),
            Err(Error::Duplicate {
                first: 0,
                second: 1,
                ..
            })
        ));
    }

    #[test]
    fn keyed_reconciliation_moves_retained_trees_and_inserts_and_drops() {
        let first = Value::string("first".into());
        let second = Value::string("second".into());
        let inserted = Value::string("inserted".into());
        let previous = Mode::Keyed(vec![first.clone(), second.clone()].into_boxed_slice());
        let next = Mode::Keyed(vec![second.clone(), inserted, first.clone()].into_boxed_slice());
        let mut trees = vec![retained(10), retained(20)];
        next.reconcile(&previous, &mut trees);

        assert_eq!(trees[0].state.downcast_ref::<Marker>().0, 20);
        assert_ne!(trees[1].tag, tree::Tag::of::<Marker>());
        assert_eq!(trees[2].state.downcast_ref::<Marker>().0, 10);

        let final_mode = Mode::Keyed(vec![second].into_boxed_slice());
        final_mode.reconcile(&next, &mut trees);
        assert_eq!(trees.len(), 1);
        assert_eq!(trees[0].state.downcast_ref::<Marker>().0, 20);
    }

    fn child(key: Option<Value>) -> widget::Value {
        let value = widget::Value::new(Stub {
            identity: widget::Identity::unique(),
        });
        match key {
            Some(key) => value.keyed(key),
            None => value,
        }
    }

    fn retained(value: usize) -> Tree {
        Tree {
            tag: tree::Tag::of::<Marker>(),
            state: tree::State::new(Marker(value)),
            translation: iced::Vector::ZERO,
            size: iced::Size::ZERO,
            children: Vec::new(),
        }
    }

    struct Marker(usize);

    struct Stub {
        identity: widget::Identity,
    }

    impl widget::Descriptor for Stub {
        fn lower(
            &self,
            _context: &widget::Context<'_>,
        ) -> iced::Element<'static, runtime::Message> {
            iced::widget::Widget::boxed(iced::widget::text("child"))
        }

        fn identity(&self) -> widget::Identity {
            self.identity.clone()
        }
    }
}
