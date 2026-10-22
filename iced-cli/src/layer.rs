//! What `rawr` hands to rawr-iced, declared as data in `manifest.toml`.

/// The manifest `rawr-iced --rawr-manifest` prints.
pub const MANIFEST: &str = include_str!("../manifest.toml");

/// What brings a program to rawr-iced.
#[must_use]
pub fn claims() -> rawr::layer::Claims {
    rawr_cli::layer::Manifest::parse(MANIFEST)
        .expect("rawr-iced's manifest is one rawr reads")
        .claims
}

#[cfg(test)]
mod tests {
    #[test]
    fn rawr_knows_rawr_iced_as_it_describes_itself() {
        let manifest = rawr_cli::layer::Manifest::parse(super::MANIFEST).unwrap();
        let known = rawr_cli::layer::catalog()
            .into_iter()
            .find(|known| known.manifest.name == manifest.name)
            .expect("rawr's catalog lists rawr-iced");
        assert_eq!(known.manifest, manifest);
        assert!(known.install.iter().any(|line| line.ends_with(" rawr-iced-cli")));
    }
}
