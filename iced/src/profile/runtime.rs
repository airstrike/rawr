//! Runner-local staged iced profile lifecycle.

use rawr_import::profile;

mod region;

pub use region::Region;

/// Side-effect-free candidate state bound to one exact generation.
pub struct Prepared {
    key: rawr_import::supervisor::GenerationKey,
    snapshot: rawr_import::surface::Snapshot,
}

/// Hidden non-input candidate with no admitted effects.
pub struct Activated {
    prepared: Prepared,
    route: profile::activation::Route,
}

/// Sole admitted iced profile owner for one runner epoch.
pub struct State {
    route: profile::activation::Route,
    snapshot: rawr_import::surface::Snapshot,
}

/// Iced's runner-local lifecycle implementation.
#[derive(Default)]
pub struct Runtime;

impl State {
    /// Return the exact admitted generation route.
    #[must_use]
    pub const fn route(&self) -> profile::activation::Route {
        self.route
    }

    /// Return the exact admitted Rawr snapshot.
    #[must_use]
    pub const fn snapshot(&self) -> rawr_import::surface::Snapshot {
        self.snapshot
    }
}

impl profile::runtime::Runtime for Runtime {
    type State = State;
    type Prepared = Prepared;
    type Activated = Activated;

    fn prepare(
        &mut self,
        context: profile::runtime::Prepare<'_>,
    ) -> Result<Prepared, profile::Error> {
        if !context.profile().has_generated_projections() {
            return Err(profile::Error::Runtime(
                "iced profile has no generated direct projection".into(),
            ));
        }
        Ok(Prepared {
            key: context.key(),
            snapshot: context.snapshot(),
        })
    }

    fn activate(
        &mut self,
        prepared: Prepared,
        route: profile::activation::Route,
    ) -> Result<Activated, profile::Error> {
        if prepared.key != route.key() {
            return Err(profile::Error::Stale);
        }
        Ok(Activated { prepared, route })
    }

    fn admit(
        &mut self,
        activated: Activated,
        route: profile::activation::Route,
    ) -> Result<State, profile::Error> {
        if activated.route != route {
            return Err(profile::Error::Stale);
        }
        Ok(State {
            route,
            snapshot: activated.prepared.snapshot,
        })
    }

    fn retire(&mut self, _state: State) {}
}
