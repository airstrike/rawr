//! API authority selected by the connected composition entry point.

use super::{Facade, Generic, Profile};
use crate::node::{self, staging::Owned};

type Three = (
    crate::api::Graph<crate::api::Extracted>,
    node::Validated<Profile>,
    node::Validated<Generic>,
    node::Validated<Facade>,
);
type Four = (
    crate::api::Graph<crate::api::Extracted>,
    node::Validated<Profile>,
    node::Validated<Generic>,
    node::Validated<Facade>,
    node::Validated<crate::checked::Product>,
);

pub(super) trait Source: Sized {
    type Phase: crate::api::Phase;
    fn graph(&self) -> &crate::api::Graph<Self::Phase>;
    fn callback_observations(&self) -> [u8; 96];
    fn context(&self) -> &crate::context::Context;
    fn retention(&self) -> Result<&node::read_set::Retention, node::Error>;
    fn input(&self) -> node::staging::Input<'_>;
    fn current(self) -> Result<crate::api::Graph<crate::api::Extracted>, node::Error>;
    fn promote(
        self,
        profile: Owned<Profile>,
        generic: Owned<Generic>,
        facade: Owned<Facade>,
        store: &crate::store::Store,
    ) -> Result<Three, node::Error>;
    fn promote_checked(
        self,
        profile: Owned<Profile>,
        generic: Owned<Generic>,
        facade: Owned<Facade>,
        checked: Owned<crate::checked::Product>,
        store: &crate::store::Store,
    ) -> Result<Four, node::Error>;
}

pub(super) struct Current {
    graph: crate::api::Graph<crate::api::Extracted>,
    chain: node::Chain,
}
impl Current {
    pub(super) fn capture(
        graph: crate::api::Graph<crate::api::Extracted>,
    ) -> Result<Self, node::Error> {
        let chain = graph.node_chain().cloned().ok_or(node::Error::State(
            "connected classification requires an API node",
        ))?;
        Ok(Self { graph, chain })
    }
}
impl Source for Current {
    type Phase = crate::api::Extracted;
    fn graph(&self) -> &crate::api::Graph<Self::Phase> {
        &self.graph
    }
    fn callback_observations(&self) -> [u8; 96] {
        self.graph
            .callback_observations(self.graph.freshness_fingerprint())
    }
    fn context(&self) -> &crate::context::Context {
        self.chain.context()
    }
    fn retention(&self) -> Result<&node::read_set::Retention, node::Error> {
        self.chain.retention().ok_or(node::Error::State(
            "connected classification requires API retention",
        ))
    }
    fn input(&self) -> node::staging::Input<'_> {
        node::staging::Input::current(self.chain.current_parent())
    }
    fn current(self) -> Result<crate::api::Graph<crate::api::Extracted>, node::Error> {
        Ok(self.graph)
    }
    fn promote(
        self,
        profile: Owned<Profile>,
        generic: Owned<Generic>,
        facade: Owned<Facade>,
        store: &crate::store::Store,
    ) -> Result<Three, node::Error> {
        let (profile, generic, facade) =
            node::staging::promote_three(profile, generic, facade, store)?;
        Ok((self.graph, profile, generic, facade))
    }
    fn promote_checked(
        self,
        profile: Owned<Profile>,
        generic: Owned<Generic>,
        facade: Owned<Facade>,
        checked: Owned<crate::checked::Product>,
        store: &crate::store::Store,
    ) -> Result<Four, node::Error> {
        let (profile, generic, facade, checked) =
            node::staging::promote_four(profile, generic, facade, checked, store)?;
        Ok((self.graph, profile, generic, facade, checked))
    }
}

#[cfg(feature = "rustc-driver")]
impl Source for crate::api::retained::Materialized {
    type Phase = crate::api::Captured;
    fn graph(&self) -> &crate::api::Graph<Self::Phase> {
        &self.graph
    }
    fn callback_observations(&self) -> [u8; 96] {
        self.publication.callback_observations(&self.graph)
    }
    fn context(&self) -> &crate::context::Context {
        &self.publication.context
    }
    fn retention(&self) -> Result<&node::read_set::Retention, node::Error> {
        Ok(&self.publication.retention)
    }
    fn input(&self) -> node::staging::Input<'_> {
        self.product.input()
    }
    fn current(self) -> Result<crate::api::Graph<crate::api::Extracted>, node::Error> {
        let Owned::Current(api) = self.product else {
            return Err(node::Error::State(
                "historical API cannot use all-current route",
            ));
        };
        self.publication.publish(self.graph, api)
    }
    fn promote(
        self,
        profile: Owned<Profile>,
        generic: Owned<Generic>,
        facade: Owned<Facade>,
        store: &crate::store::Store,
    ) -> Result<Three, node::Error> {
        let (api, profile, generic, facade) =
            node::staging::promote_four(self.product, profile, generic, facade, store)?;
        Ok((
            self.publication.publish(self.graph, api)?,
            profile,
            generic,
            facade,
        ))
    }
    fn promote_checked(
        self,
        profile: Owned<Profile>,
        generic: Owned<Generic>,
        facade: Owned<Facade>,
        checked: Owned<crate::checked::Product>,
        store: &crate::store::Store,
    ) -> Result<Four, node::Error> {
        let (api, profile, generic, facade, checked) =
            node::staging::promote_five(self.product, profile, generic, facade, checked, store)?;
        Ok((
            self.publication.publish(self.graph, api)?,
            profile,
            generic,
            facade,
            checked,
        ))
    }
}
