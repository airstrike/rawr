//! Request-local profile authority, before the actual renderer consumes inputs.

pub(crate) enum Profile {
    Current(crate::profile::Proof),
    Historical(crate::composition::profile::Pair),
}

impl From<crate::profile::Proof> for Profile {
    fn from(value: crate::profile::Proof) -> Self {
        Self::Current(value)
    }
}

impl Profile {
    pub(crate) const fn value(&self) -> &crate::profile::Proof {
        match self {
            Self::Current(value) => value,
            Self::Historical(pair) => pair.proof.value(),
        }
    }
    pub(crate) fn node_chain(&self) -> Option<&crate::node::Chain> {
        match self {
            Self::Current(value) => value.node_chain(),
            Self::Historical(_) => None,
        }
    }
    pub(crate) fn into_current(self) -> Result<crate::profile::Proof, super::request::Error> {
        match self {
            Self::Current(value) => Ok(value),
            Self::Historical(_) => Err(super::request::Error::Drift {
                field: "historical profile escaped renderer boundary",
            }),
        }
    }
}
