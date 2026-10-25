//! What `rawr` hands to rawr-iced, declared as data in the rawr-iced
//! library's `manifest.toml`.

/// The manifest `rawr-iced --rawr-manifest` prints.
pub const MANIFEST: &str = rawr_iced::MANIFEST;

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
        // The catalog says what brings a program to rawr-iced; the prelude
        // matters only to a snippet host, which reads the manifest itself.
        let mut claims = manifest.claims.clone();
        claims.prelude.clear();
        assert_eq!(known.manifest.claims, claims);
        assert_eq!(
            (
                &known.manifest.name,
                &known.manifest.extends,
                &known.manifest.templates,
                &known.manifest.commands
            ),
            (
                &manifest.name,
                &manifest.extends,
                &manifest.templates,
                &manifest.commands
            )
        );
        assert!(!manifest.claims.prelude.is_empty());
        assert!(known
            .install
            .iter()
            .any(|line| line.ends_with(" rawr-iced-cli")));
    }
}
