//! The Rust edition a crate's source is written in, as its Cargo manifest
//! names it. It decides where rustc ends a block's tail temporaries.

/// A Rust edition.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub(crate) enum Edition {
    Edition2015,
    Edition2018,
    Edition2021,
    /// The edition of a source no Cargo manifest names one for: a lone
    /// file, or a host's in-memory program.
    #[default]
    Edition2024,
}

impl Edition {
    /// The edition a Cargo manifest's `[package] edition` names: 2015 when
    /// it names none, as Cargo reads it, and the latest for one this rawr
    /// doesn't know yet.
    pub(crate) fn from_cargo(edition: Option<&str>) -> Self {
        match edition {
            None | Some("2015") => Self::Edition2015,
            Some("2018") => Self::Edition2018,
            Some("2021") => Self::Edition2021,
            Some(_) => Self::Edition2024,
        }
    }

    /// Whether a block's tail temporaries drop before the block's locals,
    /// as edition 2024 has them. Before it, they live to the end of the
    /// statement around the block, after its locals, and a borrow of a
    /// local a tail temporary's drop uses is rustc's E0597.
    pub(crate) const fn drops_tail_temporaries_first(self) -> bool {
        matches!(self, Self::Edition2024)
    }
}
