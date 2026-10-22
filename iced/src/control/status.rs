//! Structured current status for the in-process preview host.

/// The surface selected by the host controls.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Selection {
    /// The retained application is selected.
    Application,
    /// One stable preview key is selected or retained for recovery.
    Preview(rawr::preview::Key),
}

/// A read-only snapshot of current host state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Status {
    selection: Selection,
    active: bool,
    theme: Option<String>,
    diagnostics: usize,
}

impl Status {
    pub(crate) fn from_host(host: &crate::preview::host::Host) -> Self {
        Self {
            selection: match host.selection() {
                crate::preview::host::Selection::Application => Selection::Application,
                crate::preview::host::Selection::Preview(key) => Selection::Preview(key.clone()),
            },
            active: host.is_active(),
            theme: host.theme_name(),
            diagnostics: host.reports().len(),
        }
    }

    /// Return the current application/preview selection.
    #[must_use]
    pub const fn selection(&self) -> &Selection {
        &self.selection
    }

    /// Return whether the selected surface has a committed live session.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.active
    }

    /// Return the materialized native content theme, when a session is active.
    #[must_use]
    pub fn theme(&self) -> Option<&str> {
        self.theme.as_deref()
    }

    /// Return the number of canonical reports retained by the host.
    #[must_use]
    pub const fn diagnostics(&self) -> usize {
        self.diagnostics
    }
}
