//! Stable, read-only descriptions of compiler-proven previews.

/// One selectable preview exposed without its executable proofs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Preview {
    key: rawr::preview::Key,
    name: String,
    group: String,
    themes: Box<[String]>,
    snapshot: bool,
    width: u32,
    height: u32,
}

impl Preview {
    pub(crate) fn from_definition(definition: &rawr::preview::Definition) -> Self {
        Self {
            key: definition.key().clone(),
            name: definition.name().to_owned(),
            group: definition.group().to_owned(),
            themes: definition
                .themes()
                .iter()
                .map(|theme| theme.name().to_owned())
                .collect(),
            snapshot: definition.snapshot(),
            width: definition.size().width(),
            height: definition.size().height(),
        }
    }

    /// Return the opaque stable key accepted by [`super::Host::select`].
    #[must_use]
    pub const fn key(&self) -> &rawr::preview::Key {
        &self.key
    }

    /// Return the human-facing variant name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Return the human-facing group name.
    #[must_use]
    pub fn group(&self) -> &str {
        &self.group
    }

    /// Return `group/name`, the spelling `rawr preview <name>` accepts.
    #[must_use]
    pub fn label(&self) -> String {
        format!("{}/{}", self.group, self.name)
    }

    /// Whether `rawr preview <wanted>` names this preview: its name or
    /// `group/name`, ignoring ASCII case.
    #[must_use]
    pub fn matches(&self, wanted: &str) -> bool {
        !wanted.is_empty()
            && (wanted.eq_ignore_ascii_case(&self.name)
                || wanted.eq_ignore_ascii_case(&self.label()))
    }

    /// Return the declared explicit theme names in source order.
    #[must_use]
    pub fn themes(&self) -> &[String] {
        &self.themes
    }

    /// Return whether deterministic snapshot generations include this preview.
    #[must_use]
    pub const fn snapshot(&self) -> bool {
        self.snapshot
    }

    /// Return the declared logical width.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Return the declared logical height.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }
}
