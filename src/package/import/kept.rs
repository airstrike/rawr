//! What a check of the program keeps for the next check against the same
//! API: the registry it installed the crates' facades in (#1755).

use crate::{host, Shared};

/// What a compiler's checks of a program keep from one to the next, as an
/// owner that checks the program again on each save holds it. A check
/// against the API the last one installed shares that registry, where it
/// installed a thousand facades again.
#[derive(Default)]
pub struct Kept {
    installed: Option<Installed>,
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
