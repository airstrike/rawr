//! Exact Cargo core/features.rs:506–585 and the CLI diagnostic-name closure.

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Status {
    Stable,
    Unstable,
    Removed,
}

pub(super) struct Metadata {
    pub(super) name: &'static str,
    pub(super) status: Status,
    pub(super) version: &'static str,
    pub(super) docs: &'static str,
}

macro_rules! features {
    ($(($variant:ident, $name:literal, $status:ident, $version:literal, $docs:literal)),* $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub(in super::super::super) enum Feature { $($variant),* }

        const ALL: [Feature; 27] = [$(Feature::$variant),*];

        impl Feature {
            pub(super) fn metadata(self) -> Metadata {
                match self {
                    $(Self::$variant => Metadata {
                        name: $name,
                        status: Status::$status,
                        version: $version,
                        docs: $docs,
                    }),*
                }
            }

            pub(in super::super::super) fn name(self) -> &'static str {
                self.metadata().name
            }
        }
    };
}

features! {
    (TestDummyStable, "test-dummy-stable", Stable, "1.0", ""),
    (TestDummyUnstable, "test-dummy-unstable", Unstable, "", "reference/unstable.html"),
    (AlternativeRegistries, "alternative-registries", Stable, "1.34", "reference/registries.html"),
    (Edition, "edition", Stable, "1.31", "reference/manifest.html#the-edition-field"),
    (RenameDependency, "rename-dependency", Stable, "1.31", "reference/specifying-dependencies.html#renaming-dependencies-in-cargotoml"),
    (PublishLockfile, "publish-lockfile", Removed, "1.37", "reference/unstable.html#publish-lockfile"),
    (ProfileOverrides, "profile-overrides", Stable, "1.41", "reference/profiles.html#overrides"),
    (DefaultRun, "default-run", Stable, "1.37", "reference/manifest.html#the-default-run-field"),
    (Metabuild, "metabuild", Unstable, "", "reference/unstable.html#metabuild"),
    (PublicDependency, "public-dependency", Unstable, "", "reference/unstable.html#public-dependency"),
    (NamedProfiles, "named-profiles", Stable, "1.57", "reference/profiles.html#custom-profiles"),
    (Resolver, "resolver", Stable, "1.51", "reference/resolver.html#resolver-versions"),
    (Strip, "strip", Stable, "1.58", "reference/profiles.html#strip-option"),
    (RustVersion, "rust-version", Stable, "1.56", "reference/manifest.html#the-rust-version-field"),
    (Edition2021, "edition2021", Stable, "1.56", "reference/manifest.html#the-edition-field"),
    (PerPackageTarget, "per-package-target", Unstable, "", "reference/unstable.html#per-package-target"),
    (CodegenBackend, "codegen-backend", Unstable, "", "reference/unstable.html#codegen-backend"),
    (DifferentBinaryName, "different-binary-name", Unstable, "", "reference/unstable.html#different-binary-name"),
    (ProfileRustflags, "profile-rustflags", Unstable, "", "reference/unstable.html#profile-rustflags-option"),
    (WorkspaceInheritance, "workspace-inheritance", Stable, "1.64", "reference/unstable.html#workspace-inheritance"),
    (Edition2024, "edition2024", Stable, "1.85", "reference/manifest.html#the-edition-field"),
    (TrimPaths, "trim-paths", Unstable, "", "reference/unstable.html#profile-trim-paths-option"),
    (OpenNamespaces, "open-namespaces", Unstable, "", "reference/unstable.html#open-namespaces"),
    (PathBases, "path-bases", Unstable, "", "reference/unstable.html#path-bases"),
    (UnstableEditions, "unstable-editions", Unstable, "", "reference/unstable.html#unstable-editions"),
    (MultipleBuildScripts, "multiple-build-scripts", Unstable, "", "reference/unstable.html#multiple-build-scripts"),
    (PanicImmediateAbort, "panic-immediate-abort", Unstable, "", "reference/unstable.html#panic-immediate-abort"),
}

pub(super) fn find(name: &str) -> Option<Feature> {
    // Public names are already hyphen-spelled. Exact equality also rejects all
    // underscores, as pinned status() does before its temporary replacement.
    ALL.into_iter().find(|feature| feature.name() == name)
}

pub(super) fn is_cli(name: &str) -> bool {
    // Every generated CliUnstable::help entry participates, even without help.
    // Manifest names containing underscores have already taken their own error.
    const NAMES: &[&str] = &[
        "allow-features",
        "print-im-a-teapot",
        "advanced-env",
        "any-build-script-metadata",
        "asymmetric-token",
        "avoid-dev-deps",
        "binary-dep-depinfo",
        "bindeps",
        "build-analysis",
        "build-dir-new-layout",
        "build-std",
        "build-std-features",
        "cargo-lints",
        "checksum-freshness",
        "codegen-backend",
        "direct-minimal-versions",
        "dual-proc-macros",
        "feature-unification",
        "features",
        "fine-grain-locking",
        "fix-edition",
        "gc",
        "git",
        "gitoxide",
        "hint-msrv",
        "host-config",
        "json-target-spec",
        "min-publish-age",
        "minimal-versions",
        "msrv-policy",
        "mtime-on-use",
        "next-lockfile-bump",
        "no-embed-metadata",
        "no-index-update",
        "panic-abort-tests",
        "panic-immediate-abort",
        "profile-hint-mostly-unused",
        "profile-rustflags",
        "public-dependency",
        "publish-timeout",
        "root-dir",
        "rustc-unicode",
        "rustdoc-depinfo",
        "rustdoc-map",
        "rustdoc-mergeable-info",
        "rustdoc-scrape-examples",
        "sbom",
        "script",
        "section-timings",
        "separate-nightlies",
        "skip-rustdoc-fingerprint",
        "target-applies-to-host",
        "trim-paths",
        "unstable-options",
    ];
    NAMES.contains(&name)
}
