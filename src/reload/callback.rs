//! Exact callback contracts retained by one reload candidate.

use std::collections::BTreeSet;

use crate::{application, item, preview, program, type_def, widget};

/// The normalized application and preview callback proofs for one generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Manifest {
    application: Option<application::Fingerprint>,
    previews: preview::Fingerprint,
    items: Box<[item::Value]>,
    widgets: Box<[widget::Fingerprint]>,
    contracts: Contracts,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Contracts {
    application: Option<Contract>,
    previews: Box<[Preview]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Contract {
    pub(super) schemas: program::schema::Reachability,
    pub(super) callback: Fingerprint,
    pub(super) items: Box<[item::Value]>,
    pub(super) widgets: Box<[widget::Fingerprint]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Fingerprint {
    Application(Box<application::Fingerprint>),
    Preview(preview::Fingerprint),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Preview {
    key: preview::Key,
    contract: Contract,
}

impl Manifest {
    pub(crate) fn from_catalog(
        catalog: &preview::Catalog,
        schemas: &program::schema::Manifest,
    ) -> Self {
        let items = catalog.items().cloned().collect();
        let mut widgets: Vec<_> = catalog
            .widgets()
            .iter()
            .map(|definition| definition.fingerprint().clone())
            .collect();
        widgets.sort();
        let widget_roots = widget_roots(catalog.widgets());
        let application = catalog
            .application_contract()
            .map(|contract| contract.reload_fingerprint());
        let application_contract = catalog.application_contract().map(|contract| {
            let (mut roots, opaque) = contract.schema_roots();
            roots.extend(widget_roots.iter().cloned());
            let reachable = schemas.reachable(&roots, opaque);
            Contract {
                items: if reachable.is_opaque() {
                    catalog.items().cloned().collect()
                } else {
                    Default::default()
                },
                schemas: reachable,
                callback: Fingerprint::Application(Box::new(contract.reload_fingerprint())),
                widgets: widgets.clone().into_boxed_slice(),
            }
        });
        let mut preview_contracts: Vec<_> = catalog
            .previews()
            .iter()
            .map(|definition| {
                let (mut roots, opaque) = definition.schema_roots();
                roots.extend(widget_roots.iter().cloned());
                let reachable = schemas.reachable(&roots, opaque);
                Preview {
                    key: definition.key().clone(),
                    contract: Contract {
                        items: if reachable.is_opaque() {
                            catalog.items().cloned().collect()
                        } else {
                            Default::default()
                        },
                        schemas: reachable,
                        callback: Fingerprint::Preview(definition.reload_fingerprint()),
                        widgets: widgets.clone().into_boxed_slice(),
                    },
                }
            })
            .collect();
        preview_contracts.sort_by(|left, right| left.key.cmp(&right.key));
        Self {
            application,
            previews: catalog.preview_fingerprint().clone(),
            items,
            widgets: widgets.into_boxed_slice(),
            contracts: Contracts {
                application: application_contract,
                previews: preview_contracts.into_boxed_slice(),
            },
        }
    }

    /// Return the exact normalized application callback proof, when present.
    #[must_use]
    pub const fn application(&self) -> Option<&application::Fingerprint> {
        self.application.as_ref()
    }

    /// Return the exact normalized preview callback proof.
    #[must_use]
    pub const fn previews(&self) -> &preview::Fingerprint {
        &self.previews
    }

    /// Return every stable symbolic callable and exact signature proof.
    #[must_use]
    pub const fn items(&self) -> &[item::Value] {
        &self.items
    }

    /// Return every exact scripted-widget lifecycle callback proof.
    #[must_use]
    pub const fn widgets(&self) -> &[widget::Fingerprint] {
        &self.widgets
    }

    pub(super) const fn application_contract(&self) -> Option<&Contract> {
        self.contracts.application.as_ref()
    }

    pub(super) fn preview_contract(&self, key: &preview::Key) -> Option<&Contract> {
        self.contracts
            .previews
            .binary_search_by(|preview| preview.key.cmp(key))
            .ok()
            .map(|index| &self.contracts.previews[index].contract)
    }
}

fn widget_roots(definitions: &[widget::Definition]) -> BTreeSet<type_def::Identity> {
    let mut roots = BTreeSet::new();
    for definition in definitions {
        roots.insert(definition.owner().clone());
        if let Some(state) = definition.state().nominal_type() {
            state.collect_schema_roots(&mut roots);
        }
        if let Some(message) = definition.message().exact() {
            message.collect_schema_roots(&mut roots);
        }
    }
    roots
}
