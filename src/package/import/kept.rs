//! What a check of the program keeps for the next check: the registry it
//! installed the crates' facades in, for a check against the same API
//! (#1755), and what each source's check found.

use crate::{host, Shared};

/// What a compiler's checks of a program keep from one to the next, as an
/// owner that checks the program again on each save holds it. A check
/// against the API the last one installed shares that registry, where it
/// installed a thousand facades again, and lowers again only the sources
/// whose check reads something a save changed.
#[derive(Default)]
pub struct Kept {
    installed: Option<Installed>,
    lowerings: crate::resolve::lower_nominal::Lowerings,
}

/// One registry with a contract's facades installed.
struct Installed {
    /// The compiler's own registry it was installed over.
    host: Shared<host::Registry>,
    contract: rawr_import::contract::fingerprint::Fingerprint,
    api: rawr_import::api::Fingerprint,
    registry: Shared<host::Registry>,
    /// What installing it charged the check's allocation, in items and
    /// bytes, which each check that shares it charges again.
    charged: (usize, usize),
}

impl Kept {
    /// How many sources its checks lowered so far, rather than reusing an
    /// earlier check's lowering.
    #[doc(hidden)]
    #[must_use]
    pub fn checked_sources(&self) -> usize {
        self.lowerings.checked()
    }

    /// What its checks keep of each source's check, which the next check
    /// reuses.
    pub(super) fn lowerings(&mut self) -> &mut crate::resolve::lower_nominal::Lowerings {
        &mut self.lowerings
    }

    /// The registry the last check installed `contract`'s facades in over
    /// `host`, and what installing it charged.
    pub(super) fn registry(
        &self,
        host: &Shared<host::Registry>,
        contract: &rawr_import::Contract<rawr_import::Classified>,
    ) -> Option<(&Shared<host::Registry>, (usize, usize))> {
        let installed = self.installed.as_ref()?;
        (Shared::ptr_eq(&installed.host, host)
            && installed.contract == contract.fingerprint()
            && installed.api == contract.raw_fingerprint())
        .then_some((&installed.registry, installed.charged))
    }

    /// Keep `registry`, `contract`'s facades installed over `host`, for the
    /// next check, in place of the one kept before.
    pub(super) fn keep(
        &mut self,
        host: Shared<host::Registry>,
        contract: &rawr_import::Contract<rawr_import::Classified>,
        registry: Shared<host::Registry>,
        charged: (usize, usize),
    ) {
        self.installed = Some(Installed {
            host,
            contract: contract.fingerprint(),
            api: contract.raw_fingerprint(),
            registry,
            charged,
        });
    }
}
