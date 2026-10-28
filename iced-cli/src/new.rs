//! rawr-iced's `rawr new` template, `iced`: a counter in a Cargo crate that
//! depends on iced.
//! `rawr new <path> --template iced` hands the request to rawr-iced, which
//! writes it through `rawr`'s own transactional crate writer.

use std::path::PathBuf;

pub use rawr_cli::new::{Created, Error};

/// The template's name, as `rawr new --template` spells it.
pub const TEMPLATE: &str = "iced";

/// The exact `src/main.rs` installed by [`create`].
pub const COUNTER_SOURCE: &str = include_str!("../assets/counter/src/main.rs");

/// The `Cargo.toml` [`create`] writes for the package `name`. Its iced
/// names `advanced` and `sipper`, the two iced features Rawr's runner adds,
/// and every widget Rawr's facade offers, so a widget can be added while
/// the program runs and the runner builds the same iced as `cargo build`.
#[must_use]
pub fn manifest(name: &str) -> String {
    format!(
        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n\
         [dependencies]\niced = {{ git = \"https://github.com/iced-rs/iced\", rev = \"{}\", \
         features = [{}] }}\n",
        rawr_iced::native::ICED_REVISION,
        ICED_FEATURES
            .iter()
            .map(|feature| format!("\"{feature}\""))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// The iced features [`manifest`] names. The image widget comes without
/// codecs, as Rawr's runner builds it: iced's `image` would add every codec
/// the `image` crate has, AVIF's encoder among them.
pub const ICED_FEATURES: [&str; 7] = [
    "advanced",
    "canvas",
    "image-without-codecs",
    "qr_code",
    "selector",
    "sipper",
    "svg",
];

/// The template's files for the package `name`: the manifest and the
/// counter. The project's own Cargo builds Rawr's runner, so `cargo build`
/// and `rawr run` build the same iced with no other file.
#[must_use]
pub fn files(name: &str) -> Vec<rawr_cli::new::File> {
    vec![
        rawr_cli::new::File::new("Cargo.toml", manifest(name)),
        rawr_cli::new::File::new("src/main.rs", COUNTER_SOURCE),
    ]
}

/// Create the iced template's crate at `path`, resolving relative input
/// against the current directory.
pub fn create(path: PathBuf) -> Result<Created, Error> {
    rawr_cli::new::create(path, files)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn a_new_crate_is_a_cargo_toml_and_a_marked_main_rs() {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let sandbox =
            std::env::temp_dir().join(format!("rawr-iced-new-{}-{id}", std::process::id()));
        std::fs::create_dir(&sandbox).unwrap();
        let sandbox = std::fs::canonicalize(sandbox).unwrap();
        let target = sandbox.join("My Counter");

        let created = super::create(target.clone()).unwrap();

        assert_eq!(created.root(), target);
        let manifest = std::fs::read_to_string(target.join("Cargo.toml")).unwrap();
        assert_eq!(manifest, super::manifest("my-counter"));
        assert!(manifest.starts_with(
            "[package]\nname = \"my-counter\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"
        ));
        assert!(manifest.contains(&format!(
            "iced = {{ git = \"https://github.com/iced-rs/iced\", rev = \"{}\", features = [\"advanced\", \"canvas\", \"image-without-codecs\", \"qr_code\", \"selector\", \"sipper\", \"svg\"] }}",
            rawr_iced::native::ICED_REVISION
        )));
        assert!(!target.join("rust-toolchain.toml").exists());
        assert!(!target.join(".cargo").exists());
        let source = std::fs::read_to_string(target.join("src/main.rs")).unwrap();
        assert_eq!(source, super::COUNTER_SOURCE);
        assert!(rawr::project::is_marked(&source));
        assert!(matches!(
            rawr::project::Target::discover(&target).unwrap(),
            rawr::project::Target::Project(_)
        ));
        // `rawr check` passes on it; `cargo build` is checked by hand, as it
        // fetches iced.
        #[cfg(feature = "native-runner")]
        assert_eq!(
            crate::check::execute(Some(target.clone())).unwrap(),
            target.join("src/main.rs")
        );
        std::fs::remove_dir_all(&sandbox).unwrap();
    }
}
