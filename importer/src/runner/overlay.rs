//! Historical generated-overlay evidence, completed only by the runner union.

use crate::node;

/// Owned ancestry is not a current parent or an execution capability.
pub(crate) struct Overlay {
    context: crate::context::Context,
    fragment: node::Fragment,
    retention: Option<node::read_set::Retention>,
    #[cfg(test)]
    action: node::ActionKey,
}

impl Overlay {
    pub(crate) fn from_current(chain: node::Chain) -> Result<Self, node::Error> {
        chain.parent().read_set().charge_copy()?;
        Ok(Self {
            context: chain.context().clone(),
            fragment: chain.parent().read_set().clone(),
            retention: chain
                .retention()
                .map(|value| value.try_clone())
                .transpose()?,
            #[cfg(test)]
            action: chain.parent().action(),
        })
    }

    pub(crate) fn retain<K: node::Value>(
        chain: &node::Chain,
        value: &node::staging::Owned<K>,
    ) -> Result<Self, node::Error> {
        let initial = chain.retention().ok_or(node::Error::State(
            "historical overlay requires retained ancestry",
        ))?;
        let (fragment, _action, retention) = match value {
            node::staging::Owned::Current(value) => (
                value.receipt().read_set(),
                value.receipt().action(),
                initial.extend_current(value)?,
            ),
            node::staging::Owned::Staged(value) => (
                value.fragment(),
                value.action(),
                initial.extend_staged(value)?,
            ),
        };
        fragment.charge_copy()?;
        Ok(Self {
            context: chain.context().clone(),
            fragment: fragment.clone(),
            retention: Some(retention),
            #[cfg(test)]
            action: _action,
        })
    }

    pub(crate) fn context(&self) -> &crate::context::Context {
        &self.context
    }

    pub(crate) fn fragment(&self) -> &node::Fragment {
        &self.fragment
    }

    pub(crate) fn retention(&self) -> Option<&node::read_set::Retention> {
        self.retention.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn action(&self) -> node::ActionKey {
        self.action
    }
}

pub(crate) fn prepare_union(
    runner: &node::Chain,
    overlay: Option<&Overlay>,
) -> Result<node::read_set::Prepared, node::Error> {
    if let Some(overlay) = overlay
        && runner.context().identity() != overlay.context().identity()
    {
        return Err(node::Error::State("runner and overlay contexts differ"));
    }
    let retention = match (runner.retention(), overlay) {
        (Some(runner), Some(overlay)) => overlay
            .retention()
            .map(|overlay| runner.union(overlay))
            .transpose()?,
        (Some(runner), None) => Some(runner.try_clone()?),
        (None, _) => None,
    };
    let prepared = node::read_set::Prepared::seal_union(
        runner.context().clone(),
        std::iter::once(runner.parent().read_set().clone())
            .chain(overlay.map(|overlay| overlay.fragment().clone())),
    )?;
    match retention {
        Some(retention) => prepared.with_retention(retention),
        None => Ok(prepared),
    }
}
