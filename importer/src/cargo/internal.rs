//! Exact internal Rawr dependency and package-profile authority.

use std::path::{Path, PathBuf};

use toml_edit::{value, DocumentMut, Item, Table};

use super::config;
use super::unit;
use super::{canonical_directory, read_bounded_budgeted, RequestAllocation, Selection};
use crate::{Error, Result};

const SCHEMA: u32 = 1;
const PACKAGE: &str = "rawr";
const FEATURE: &str = "std";

#[cfg(test)]
std::thread_local! {
    static OWNED_SEEDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static OWNED_RECIPES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Rawr-root evidence transformed before the final Cargo manifest exists.
pub(crate) struct Seed {
    root: PathBuf,
    version: Box<str>,
    /// Rawr's own `[patch]` table (paths absolute), the patches its
    /// facade's framework revision genuinely needs.
    patch: Option<Item>,
}

/// Exact Cargo metadata transformed into the sealed internal-build recipe.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct Recipe {
    package_id: Box<str>,
    source: Box<str>,
    root: PathBuf,
    version: Box<str>,
    _allocation: RequestAllocation,
}

impl Seed {
    pub(crate) fn transform(root: &Path, allocation: &mut RequestAllocation) -> Result<Self> {
        allocation.path(root)?;
        let root = canonical_directory("internal Rawr root", root)?;
        allocation.retained(
            1,
            root.as_os_str().len().saturating_add("/Cargo.toml".len()),
        )?;
        let manifest = root.join("Cargo.toml");
        // The file read is I/O retention bounded by the physical tail
        // constant (manifest class); the manifest target is charged below on
        // the request account.
        let bytes = read_bounded_budgeted(
            "internal Rawr manifest",
            &manifest,
            crate::process::TAIL_BYTES,
            allocation,
        )?;
        allocation.retained(bytes.len(), bytes.len())?;
        let text = std::str::from_utf8(&bytes).map_err(|error| Error::Schema {
            document: "internal Rawr manifest",
            reason: error.to_string().into_boxed_str(),
        })?;
        let document = text.parse::<DocumentMut>().map_err(|error| Error::Schema {
            document: "internal Rawr manifest",
            reason: error.to_string().into_boxed_str(),
        })?;
        let package = document
            .get("package")
            .and_then(Item::as_table_like)
            .ok_or_else(|| Error::Schema {
                document: "internal Rawr manifest",
                reason: "package table is absent".into(),
            })?;
        if package.get("name").and_then(Item::as_str) != Some(PACKAGE) {
            return Err(Error::Schema {
                document: "internal Rawr manifest",
                reason: "package identity is not `rawr`".into(),
            });
        }
        let version = package
            .get("version")
            .and_then(Item::as_str)
            .filter(|version| !version.is_empty())
            .ok_or_else(|| Error::Schema {
                document: "internal Rawr manifest",
                reason: "package version is absent".into(),
            })?;
        let has_std = document
            .get("features")
            .and_then(Item::as_table_like)
            .is_some_and(|features| features.contains_key(FEATURE));
        if !has_std {
            return Err(Error::Schema {
                document: "internal Rawr manifest",
                reason: "required `std` feature is absent".into(),
            });
        }
        allocation.string(version)?;
        let patch = match document.get("patch") {
            Some(item) => {
                let mut item = item.clone();
                super::absolutize_policy_paths(&mut item, &root, allocation)?;
                Some(item)
            }
            None => None,
        };
        #[cfg(test)]
        OWNED_SEEDS.with(|owned| owned.set(owned.get().saturating_add(1)));
        Ok(Self {
            root,
            version: version.into(),
            patch,
        })
    }

    /// Rawr's own `[patch]` table, if it has one.
    pub(crate) fn patch(&self) -> Option<&Item> {
        self.patch.as_ref()
    }

    pub(crate) fn dependency(&self, limits: &super::Limits) -> Result<config::Dependency> {
        config::Dependency::rawr(&self.root, limits)
    }

    pub(crate) fn seal_accounted(
        &self,
        metadata: &unit::Metadata,
        selected: &[Selection],
        allocation: &mut RequestAllocation,
    ) -> Result<Recipe> {
        let internal = selected
            .iter()
            .find(|selection| selection.alias() == config::INTERNAL_ALIAS)
            .ok_or(Error::ResolutionDrift {
                command: "metadata",
                field: "internal Rawr selection",
            })?;
        if selected.iter().any(|selection| {
            selection.alias() != config::INTERNAL_ALIAS
                && selection.package_id() == internal.package_id()
        }) {
            return Err(Error::Request {
                field: "internal Rawr package identity",
                reason: "a caller dependency resolves to the reserved internal Rawr PackageId"
                    .into(),
            });
        }
        let package = metadata
            .package(internal.package_id())
            .ok_or(Error::ResolutionDrift {
                command: "metadata",
                field: "internal Rawr package authority",
            })?;
        let unit::PackageSource::Path { cargo_id } = package.source() else {
            return Err(Error::ResolutionDrift {
                command: "metadata",
                field: "internal Rawr path source",
            });
        };
        if package.name() != PACKAGE
            || package.version() != self.version.as_ref()
            || package.root() != self.root
        {
            return Err(Error::ResolutionDrift {
                command: "metadata",
                field: "internal Rawr exact package identity",
            });
        }
        Recipe::transform_accounted(package, cargo_id, allocation)
    }
}

impl Recipe {
    fn transform_accounted(
        package: &unit::Package,
        cargo_id: &str,
        allocation: &mut RequestAllocation,
    ) -> Result<Self> {
        allocation.string(package.id())?;
        allocation.string(cargo_id)?;
        allocation.path(package.root())?;
        allocation.string(package.version())?;
        #[cfg(test)]
        OWNED_RECIPES.with(|owned| owned.set(owned.get().saturating_add(1)));
        Ok(Self {
            package_id: package.id().into(),
            source: cargo_id.into(),
            root: package.root().to_path_buf(),
            version: package.version().into(),
            _allocation: *allocation,
        })
    }

    pub(crate) const fn schema(&self) -> u32 {
        SCHEMA
    }

    pub(crate) fn package_id(&self) -> &str {
        &self.package_id
    }

    pub(crate) fn source(&self) -> &str {
        &self.source
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn version(&self) -> &str {
        &self.version
    }

    pub(crate) fn revalidate(
        &self,
        metadata: &unit::Metadata,
        selected: &[Selection],
    ) -> Result<()> {
        let selection = selected
            .iter()
            .find(|selection| selection.alias() == config::INTERNAL_ALIAS)
            .ok_or(Error::ResolutionDrift {
                command: "publication validation",
                field: "internal Rawr selection",
            })?;
        if selection.package_id() != self.package_id.as_ref() {
            return Err(Error::ResolutionDrift {
                command: "publication validation",
                field: "internal Rawr PackageId",
            });
        }
        let package = metadata
            .package(selection.package_id())
            .ok_or(Error::ResolutionDrift {
                command: "publication validation",
                field: "internal Rawr package authority",
            })?;
        let unit::PackageSource::Path { cargo_id } = package.source() else {
            return Err(Error::ResolutionDrift {
                command: "publication validation",
                field: "internal Rawr path source",
            });
        };
        if package.name() != PACKAGE
            || package.version() != self.version.as_ref()
            || package.root() != self.root
            || cargo_id.as_ref() != self.source.as_ref()
        {
            return Err(Error::ResolutionDrift {
                command: "publication validation",
                field: "internal Rawr exact package identity",
            });
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn fixture() -> Self {
        Self {
            package_id: "path+file:///exact/rawr#rawr@1.23.6".into(),
            source: "path+file:///exact/rawr".into(),
            root: PathBuf::from("/exact/rawr"),
            version: "1.23.6".into(),
            _allocation: super::RequestAllocation::new(&super::Limits::standard()),
        }
    }

    #[cfg(test)]
    pub(crate) fn apply_profile(&self, document: &mut DocumentMut, profile: &str) -> Result<()> {
        let mut allocation = super::RequestAllocation::new(&super::Limits::standard());
        self.apply_profile_budgeted(document, profile, &mut allocation)
    }

    pub(crate) fn apply_profile_budgeted(
        &self,
        document: &mut DocumentMut,
        profile: &str,
        allocation: &mut super::RequestAllocation,
    ) -> Result<()> {
        allocation.items(9)?;
        for value in [
            "profile",
            profile,
            "package",
            self.package_id.as_ref(),
            "debug",
            "opt-level",
            "strip",
            "build-override",
            "strip",
        ] {
            allocation.string(value)?;
        }
        let profiles = table(
            document.as_table_mut(),
            "profile",
            "Cargo profile authority",
        )?;
        let selected = table(profiles, profile, "selected Cargo profile")?;
        let packages = table(selected, "package", "Cargo package profile authority")?;
        let short = allocation.formatted(format_args!("{PACKAGE}@{}", self.version))?;
        let collides = packages.iter().any(|(selector, _)| {
            selector == PACKAGE || selector == short || selector == self.package_id.as_ref()
        });
        if collides {
            return Err(Error::Request {
                field: "internal Rawr package profile",
                reason: format!(
                    "selected profile `{profile}` already overrides the reserved Rawr identity"
                )
                .into_boxed_str(),
            });
        }
        // The embedded compiler runs on every Rawr edit; optimize it like the
        // released CLI so an edit recompiles in milliseconds. User crates keep
        // the selected profile's own semantics.
        let mut override_table = Table::new();
        override_table.insert("debug", value(0));
        override_table.insert("opt-level", value(3));
        // Without debug info Cargo strips it with `rust-objcopy`, which some
        // toolchains ship broken. Nothing is stripped, so the build never
        // runs it.
        override_table.insert("strip", value(false));
        packages.insert(self.package_id.as_ref(), Item::Table(override_table));
        let selected = table(profiles_of(document)?, profile, "selected Cargo profile")?;
        let build = table(selected, "build-override", "Cargo build script profile")?;
        if !build.contains_key("strip") {
            build.insert("strip", value(false));
        }
        Ok(())
    }
}

fn profiles_of(document: &mut DocumentMut) -> Result<&mut Table> {
    table(document.as_table_mut(), "profile", "Cargo profile authority")
}

fn table<'a>(parent: &'a mut Table, key: &str, document: &'static str) -> Result<&'a mut Table> {
    if !parent.contains_key(key) {
        parent.insert(key, Item::Table(Table::new()));
    }
    parent
        .get_mut(key)
        .and_then(Item::as_table_mut)
        .ok_or_else(|| Error::Schema {
            document,
            reason: format!("`{key}` is not a table").into_boxed_str(),
        })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use toml_edit::{DocumentMut, Item};

    use super::{Recipe, Seed, OWNED_RECIPES, OWNED_SEEDS};
    use crate::cargo::unit::{Package, PackageSource};

    fn recipe() -> Recipe {
        Recipe::fixture()
    }

    /// A target one unit under an exact total refuses nothing; the
    /// account records the axis with that target and a high water equal to
    /// the exact total the exercise charged.
    fn assert_exceeded(
        record: crate::exceedance::Record,
        name: &str,
        target: usize,
        high_water: usize,
    ) {
        let axis = record
            .axis(name)
            .unwrap_or_else(|| panic!("{name} records the lowered target"));
        assert_eq!(axis.target, target as u64);
        assert_eq!(axis.high_water, high_water as u64);
        assert!(axis.first_requested > axis.target);
        assert!(axis.first_requested <= axis.high_water);
    }

    #[test]
    fn seed_and_recipe_identity_use_exact_caps_before_ownership() {
        let root = std::env::temp_dir().join(format!(
            "rawr-internal-ledger-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir(&root).expect("internal fixture root is created");
        fs::write(
            root.join("Cargo.toml"),
            b"[package]\nname = \"rawr\"\nversion = \"1.23.6\"\n[features]\nstd = []\n",
        )
        .expect("internal fixture manifest is written");
        let root = fs::canonicalize(root).expect("internal fixture root canonicalizes");
        let seed_exercise = |limits: &crate::cargo::Limits| {
            let mut allocation = crate::cargo::RequestAllocation::new(limits);
            OWNED_SEEDS.with(|owned| owned.set(0));
            let seed = Seed::transform(&root, &mut allocation)?;
            Ok::<_, crate::Error>((allocation, seed))
        };
        let standard = crate::cargo::Limits::standard();
        let (usage, _) = seed_exercise(&standard).expect("standard Seed ledger measures");
        let mut exact = standard;
        exact.items = usage.items;
        exact.manifest_bytes = usage.bytes;
        let (exact_usage, _) = seed_exercise(&exact).expect("exact Seed ledger succeeds");
        assert_eq!(OWNED_SEEDS.with(std::cell::Cell::get), 1);
        assert!(!exact_usage.exceedances().is_exceeded());
        // One unit under the exact Seed ledger refuses nothing and still
        // owns the seed; the account records the lowered axis at the exact total.
        let mut item_floor = exact;
        item_floor.items = item_floor.items.saturating_sub(1);
        let (item_usage, _) =
            seed_exercise(&item_floor).expect("one item under the Seed ledger still transforms");
        assert_eq!(OWNED_SEEDS.with(std::cell::Cell::get), 1);
        assert_exceeded(
            item_usage.exceedances(),
            crate::cargo::REQUEST_ITEMS_RESOURCE,
            item_floor.items,
            usage.items,
        );
        let mut byte_floor = exact;
        byte_floor.manifest_bytes = byte_floor.manifest_bytes.saturating_sub(1);
        let (byte_usage, _) =
            seed_exercise(&byte_floor).expect("one byte under the Seed ledger still transforms");
        assert_eq!(OWNED_SEEDS.with(std::cell::Cell::get), 1);
        assert_exceeded(
            byte_usage.exceedances(),
            crate::cargo::REQUEST_BYTES_RESOURCE,
            byte_floor.manifest_bytes,
            usage.bytes,
        );

        let source = format!("path+file://{}", root.display());
        let id = format!("{source}#rawr@1.23.6");
        let package = Package::fixture(
            &id,
            "rawr",
            "1.23.6",
            root.clone(),
            PackageSource::Path {
                cargo_id: source.clone().into(),
            },
        );
        let recipe_exercise = |limits: &crate::cargo::Limits| {
            OWNED_RECIPES.with(|owned| owned.set(0));
            Recipe::transform_accounted(
                &package,
                &source,
                &mut crate::cargo::RequestAllocation::new(limits),
            )
        };
        let recipe = recipe_exercise(&standard).expect("standard Recipe ledger measures");
        let usage = recipe._allocation;
        let mut exact = standard;
        exact.items = usage.items;
        exact.manifest_bytes = usage.bytes;
        let exact_recipe = recipe_exercise(&exact).expect("exact Recipe ledger succeeds");
        assert_eq!(OWNED_RECIPES.with(std::cell::Cell::get), 1);
        assert!(!exact_recipe._allocation.exceedances().is_exceeded());
        // One unit under the exact Recipe ledger refuses nothing and
        // still owns the recipe; its account records the lowered axis.
        let mut item_floor = exact;
        item_floor.items = item_floor.items.saturating_sub(1);
        let item_recipe = recipe_exercise(&item_floor)
            .expect("one item under the Recipe ledger still transforms");
        assert_eq!(OWNED_RECIPES.with(std::cell::Cell::get), 1);
        assert_exceeded(
            item_recipe._allocation.exceedances(),
            crate::cargo::REQUEST_ITEMS_RESOURCE,
            item_floor.items,
            usage.items,
        );
        let mut byte_floor = exact;
        byte_floor.manifest_bytes = byte_floor.manifest_bytes.saturating_sub(1);
        let byte_recipe = recipe_exercise(&byte_floor)
            .expect("one byte under the Recipe ledger still transforms");
        assert_eq!(OWNED_RECIPES.with(std::cell::Cell::get), 1);
        assert_exceeded(
            byte_recipe._allocation.exceedances(),
            crate::cargo::REQUEST_BYTES_RESOURCE,
            byte_floor.manifest_bytes,
            usage.bytes,
        );
        fs::remove_dir_all(root).expect("internal fixture is removed");
    }

    #[test]
    fn exact_package_id_receives_only_debug_zero_and_optimization_in_selected_profile() {
        let mut document = "[profile.dev]\nopt-level = 2\ndebug-assertions = false\noverflow-checks = false\ncodegen-units = 8\n"
            .parse::<DocumentMut>()
            .expect("profile fixture parses");
        recipe()
            .apply_profile(&mut document, "dev")
            .expect("exact internal override seals");
        let profile = document["profile"]["dev"]
            .as_table()
            .expect("selected profile remains a table");
        assert_eq!(profile.get("opt-level").and_then(Item::as_integer), Some(2));
        assert_eq!(
            profile.get("debug-assertions").and_then(Item::as_bool),
            Some(false)
        );
        assert_eq!(
            profile.get("overflow-checks").and_then(Item::as_bool),
            Some(false)
        );
        assert_eq!(
            profile.get("codegen-units").and_then(Item::as_integer),
            Some(8)
        );
        let packages = profile["package"]
            .as_table()
            .expect("package override table exists");
        assert_eq!(packages.len(), 1);
        let internal = packages["path+file:///exact/rawr#rawr@1.23.6"]
            .as_table()
            .expect("fully qualified PackageId is the selector");
        assert_eq!(internal.len(), 3);
        assert_eq!(internal["debug"].as_integer(), Some(0));
        assert_eq!(internal["opt-level"].as_integer(), Some(3));
        // Nothing is stripped, so no build runs `rust-objcopy`.
        assert_eq!(internal["strip"].as_bool(), Some(false));
        assert_eq!(profile["build-override"]["strip"].as_bool(), Some(false));
    }

    #[test]
    fn release_and_custom_profiles_keep_unrelated_semantics() {
        for profile in ["release", "fast"] {
            let mut document = format!(
                "[profile.{profile}]\nopt-level = 3\nlto = \"thin\"\npanic = \"abort\"\ncodegen-units = 4\n"
            )
            .parse::<DocumentMut>()
            .expect("custom profile fixture parses");
            recipe()
                .apply_profile(&mut document, profile)
                .expect("internal override applies to selected profile");
            let selected = document["profile"][profile]
                .as_table()
                .expect("selected profile remains a table");
            assert_eq!(selected["opt-level"].as_integer(), Some(3));
            assert_eq!(selected["lto"].as_str(), Some("thin"));
            assert_eq!(selected["panic"].as_str(), Some("abort"));
            assert_eq!(selected["codegen-units"].as_integer(), Some(4));
            assert_eq!(
                selected["package"]["path+file:///exact/rawr#rawr@1.23.6"]["debug"].as_integer(),
                Some(0)
            );
        }
    }

    #[test]
    fn reserved_override_collisions_reject_but_other_source_remains_distinct() {
        for selector in ["rawr", "rawr@1.23.6", "path+file:///exact/rawr#rawr@1.23.6"] {
            let mut document = format!("[profile.dev.package.\"{selector}\"]\ndebug = 1\n")
                .parse::<DocumentMut>()
                .expect("collision fixture parses");
            assert!(recipe().apply_profile(&mut document, "dev").is_err());
        }

        let other = "path+file:///other/rawr#rawr@1.23.6";
        let mut document = format!("[profile.dev.package.\"{other}\"]\ndebug = 1\n")
            .parse::<DocumentMut>()
            .expect("source-distinct fixture parses");
        recipe()
            .apply_profile(&mut document, "dev")
            .expect("same name/version at a different source is not conflated");
        let packages = document["profile"]["dev"]["package"]
            .as_table()
            .expect("both source-qualified selectors remain");
        assert_eq!(packages.len(), 2);
        assert_eq!(packages[other]["debug"].as_integer(), Some(1));
        assert_eq!(
            packages["path+file:///exact/rawr#rawr@1.23.6"]["debug"].as_integer(),
            Some(0)
        );
    }
}
