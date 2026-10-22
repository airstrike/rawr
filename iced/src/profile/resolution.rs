//! Rawr-owned iced resolution universe.

use rawr_import::cargo::{self, Limits};
pub(super) fn catalog(limits: &Limits) -> rawr_import::Result<cargo::universe::Catalog> {
    cargo::universe::Catalog::iced_current_v2(limits)
}

#[cfg(test)]
mod tests {
    #[test]
    fn reports_the_authenticated_fixture_identity() {
        let limits = rawr_import::cargo::Limits::standard();
        let universe = super::catalog(&limits).expect("universe seals");
        assert_ne!(universe.identity(), &[0; 32]);
    }
}
