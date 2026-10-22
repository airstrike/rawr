//! Tests of Selection data only. Real Cargo matching, unique workspace
//! selection, and command success belong to later contained implementation and
//! product-seam tests; the pinned-source citations are rationale, not acceptance.

use std::path::{Path, PathBuf};

use super::{Basis, Selection, Source, OWNED_BUFFERS};
use crate::cargo::{toolchain, unit};
use crate::input::{
    Allocation, Usage, INPUT_BYTES_RESOURCE, INPUT_ITEMS_RESOURCE, INPUT_STRING_RESOURCE,
};
use crate::Error;

const OID: &str = "0123456789012345678901234567890123456789";
const OTHER_OID: &str = "1123456789012345678901234567890123456789";

fn path(name: &str, version: &str, root: &str) -> unit::Package {
    let source = format!("path+file://{root}");
    path_source(name, version, root, &source)
}

fn path_source(name: &str, version: &str, root: &str, source: &str) -> unit::Package {
    unit::Package::fixture(
        &format!("{source}#{name}@{version}"),
        name,
        version,
        PathBuf::from(root),
        unit::PackageSource::Path {
            cargo_id: source.into(),
        },
    )
}

fn git(name: &str, version: &str, root: &str) -> unit::Package {
    unit::Package::fixture(
        &format!("git+https://example.test/repository?branch=main#{name}@{version}"),
        name,
        version,
        PathBuf::from(root),
        unit::PackageSource::Git {
            cargo_id: format!("git+https://example.test/repository?branch=main#{OID}").into(),
            oid: OID.into(),
        },
    )
}

fn allocation() -> Allocation {
    Allocation::with_limits(usize::MAX, usize::MAX, usize::MAX)
}

fn selection(package: &unit::Package) -> Selection {
    Selection::derive(
        toolchain::supported_cargo_commit(),
        package,
        package.manifest(),
        &mut allocation(),
    )
    .expect("authenticated fixture derives selector data")
}

#[test]
fn raw_selector_keeps_normalized_name_and_full_version() {
    let package = path("renamed", "1.2.3-rc.4+build.7", "/work/directory");
    let derived = selection(&package);
    assert_eq!(derived.schema(), 1);
    assert_eq!(derived.selector(), "renamed@1.2.3-rc.4+build.7");
    assert_eq!(derived.package_id(), package.id());
    assert_eq!(derived.local().name(), "renamed");
    assert_eq!(derived.local().version(), "1.2.3-rc.4+build.7");
    assert_eq!(derived.local().root(), Path::new("/work/directory"));
    assert_eq!(derived.local().manifest(), package.manifest());
    assert_eq!(derived.local().source_path(), package.root());
    assert!(derived.matches_package(&package));
}

#[test]
fn path_and_git_share_command_predicate_but_keep_distinct_owners() {
    let local = path("owner", "1.2.3", "/work/checkout");
    let imported = git("owner", "1.2.3", "/work/checkout");
    let local_data = selection(&local);
    let imported_data = selection(&imported);
    assert_eq!(local_data.selector(), imported_data.selector());
    assert_eq!(
        local_data.local().source_path(),
        imported_data.local().source_path(),
    );
    assert_ne!(local_data.package_id(), imported_data.package_id());
    assert!(!local_data.matches_package(&imported));
    assert!(!imported_data.matches_package(&local));
    let Source::Git { cargo_id, oid } = &imported_data.source else {
        panic!("the full Git source remains independently retained")
    };
    assert_eq!(cargo_id, imported.source().cargo_id());
    assert_eq!(oid, OID);
}

#[test]
fn unicode_names_roots_and_namespace_spelling_are_independent() {
    for (name, root, source) in [
        ("π", "/work/ascii", "path+file:///work/ascii"),
        ("ascii", "/work/π", "path+file:///work/%CF%80"),
        ("π", "/work/π", "path+file:///work/%CF%80"),
        (
            "namespace::member",
            "/work/unrelated",
            "path+file:///work/unrelated",
        ),
        ("χώρος::μέλος", "/work/根", "path+file:///work/%E6%A0%B9"),
    ] {
        let package = path_source(name, "1.2.3", root, source);
        let derived = selection(&package);
        let (parsed_name, parsed_version) = derived
            .selector()
            .rsplit_once('@')
            .expect("raw PackageIdSpec uses the final at sign");
        assert_eq!(parsed_name, name);
        assert_eq!(parsed_version, "1.2.3");
        assert_eq!(derived.local().root(), Path::new(root));
        assert!(!derived.selector().contains("://"));
        // Namespace and actual target eligibility are existing manifest/CLI
        // gates. This fixture does not supply an opt-in or claim build success.
        assert!(derived.matches_package(&package));
    }
}

#[test]
fn roots_never_render_into_or_modify_the_raw_selector() {
    for (root, source) in [
        ("/", "path+file:///"),
        ("/work/name/", "path+file:///work/name"),
        ("/work/.hidden/..name", "path+file:///work/.hidden/..name"),
        ("/work/%#?[] space", "path+file:///work/%25%23%3F[]%20space"),
    ] {
        let package = path_source("owner", "1.2.3", root, source);
        let derived = selection(&package);
        assert_eq!(derived.selector(), "owner@1.2.3");
        assert_eq!(
            derived.local().root().as_os_str(),
            package.root().as_os_str()
        );
        assert_eq!(
            derived.local().manifest().as_os_str(),
            package.manifest().as_os_str(),
        );
    }
}

#[cfg(unix)]
#[test]
fn native_byte_root_is_owned_without_lossy_conversion() {
    use std::ffi::OsString;
    use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};

    let root = PathBuf::from(OsString::from_vec(b"/work/\xff\x80%#?".to_vec()));
    let package = unit::Package::fixture(
        "path+file:///work/%FF%80%25%23%3F#owner@1.2.3",
        "owner",
        "1.2.3",
        root,
        unit::PackageSource::Path {
            cargo_id: "path+file:///work/%FF%80%25%23%3F".into(),
        },
    );
    let derived = selection(&package);
    assert_eq!(derived.selector(), "owner@1.2.3");
    assert_eq!(
        derived.local().source_path().as_os_str().as_bytes(),
        b"/work/\xff\x80%#?"
    );
    assert!(derived.matches_package(&package));
}

#[test]
fn expected_path_source_keeps_the_upstream_embedded_manifest_branch() {
    for manifest in ["/work/Cargo.toml", "/work/other.toml", "/work/source.RS"] {
        assert!(matches!(
            Basis::from_manifest(Path::new(manifest)),
            Basis::Root
        ));
    }
    for manifest in ["/work/source.rs", "/work/script"] {
        assert!(matches!(
            Basis::from_manifest(Path::new(manifest)),
            Basis::Manifest
        ));
    }
    // The actual package CLI separately rejects an embedded *workspace root*
    // before selection. Testing this source-basis rule grants no CLI success.
}

#[test]
fn wrong_manifest_pin_and_registry_route_fail_before_retention() {
    let package = path("owner", "1.2.3", "/work/owner");
    OWNED_BUFFERS.with(|owned| owned.set(0));
    assert!(matches!(
        Selection::derive("other", &package, package.manifest(), &mut allocation()),
        Err(Error::Input {
            code: "package-selector-cargo",
            ..
        }),
    ));
    assert!(matches!(
        Selection::derive(
            toolchain::supported_cargo_commit(),
            &package,
            Path::new("/work/foreign/Cargo.toml"),
            &mut allocation(),
        ),
        Err(Error::Input {
            code: "package-selector-manifest",
            ..
        }),
    ));
    let registry = unit::Package::fixture(
        "registry+https://example.test/index#owner@1.2.3",
        "owner",
        "1.2.3",
        PathBuf::from("/registry/owner"),
        unit::PackageSource::Registry {
            cargo_id: "registry+https://example.test/index".into(),
            checksum: Some([1; 32]),
        },
    );
    assert!(matches!(
        Selection::derive(
            toolchain::supported_cargo_commit(),
            &registry,
            registry.manifest(),
            &mut allocation(),
        ),
        Err(Error::Input {
            code: "package-selector-source",
            ..
        }),
    ));
    assert_eq!(OWNED_BUFFERS.with(std::cell::Cell::get), 0);
}

#[test]
fn the_original_join_compares_each_owner_field_including_source_and_oid() {
    let package = git("owner", "1.2.3", "/work/owner");
    let derived = selection(&package);
    let swapped = |id: &str, name: &str, version: &str, root: &Path, source| {
        unit::Package::fixture(id, name, version, root.to_path_buf(), source)
    };
    for foreign in [
        swapped(
            "other-id",
            package.name(),
            package.version(),
            package.root(),
            package.source().clone(),
        ),
        swapped(
            package.id(),
            "other",
            package.version(),
            package.root(),
            package.source().clone(),
        ),
        swapped(
            package.id(),
            package.name(),
            "1.2.3+foreign",
            package.root(),
            package.source().clone(),
        ),
        swapped(
            package.id(),
            package.name(),
            package.version(),
            Path::new("/work/foreign"),
            package.source().clone(),
        ),
        swapped(
            package.id(),
            package.name(),
            package.version(),
            package.root(),
            unit::PackageSource::Git {
                cargo_id: format!("git+https://example.test/foreign#{OID}").into(),
                oid: OID.into(),
            },
        ),
        swapped(
            package.id(),
            package.name(),
            package.version(),
            package.root(),
            unit::PackageSource::Git {
                cargo_id: package.source().cargo_id().into(),
                oid: OTHER_OID.into(),
            },
        ),
        swapped(
            package.id(),
            package.name(),
            package.version(),
            package.root(),
            unit::PackageSource::Path {
                cargo_id: package.source().cargo_id().into(),
            },
        ),
    ] {
        assert!(!derived.matches_package(&foreign));
    }
    let mut changed_manifest = selection(&package);
    changed_manifest.local.manifest = PathBuf::from("/work/owner/other.toml");
    assert!(!changed_manifest.matches_package(&package));
}

#[test]
fn full_version_spelling_and_owner_equality_preserve_prerelease_and_build_metadata() {
    for (version, selector, candidate, equal_owner) in [
        ("1.2.3", "owner@1.2.3", "1.2.3", true),
        ("1.2.3", "owner@1.2.3", "1.2.3+build", false),
        ("1.2.3+build", "owner@1.2.3+build", "1.2.3", false),
        ("1.2.3+build", "owner@1.2.3+build", "1.2.3+other", false),
        ("1.2.3+build", "owner@1.2.3+build", "1.2.3+build", true),
        ("1.2.3", "owner@1.2.3", "1.2.3-rc.1", false),
        ("1.2.3-rc.1", "owner@1.2.3-rc.1", "1.2.3-rc.1+build", false),
        ("1.2.3-rc.1", "owner@1.2.3-rc.1", "1.2.3-rc.2", false),
        (
            "1.2.3-rc.1+build",
            "owner@1.2.3-rc.1+build",
            "1.2.3-rc.1+build",
            true,
        ),
    ] {
        let owner = path("owner", version, "/work/owner");
        let derived = selection(&owner);
        assert_eq!(derived.selector(), selector);
        assert_eq!(derived.local().version(), version);
        // Isolate the exact version field by retaining every other owner field.
        // This deliberately altered fixture is not authenticated metadata.
        let altered = unit::Package::fixture(
            owner.id(),
            owner.name(),
            candidate,
            owner.root().to_path_buf(),
            owner.source().clone(),
        );
        assert_eq!(derived.matches_package(&altered), equal_owner);
    }
}

fn retained(package: &unit::Package) -> (Usage, usize) {
    let mut strings = vec![
        package.id(),
        package.name(),
        package.version(),
        package.source().cargo_id(),
    ];
    if let unit::PackageSource::Git { oid, .. } = package.source() {
        strings.push(oid);
    }
    let selector_len = package.name().len() + 1 + package.version().len();
    let maximum = strings
        .iter()
        .map(|value| value.len())
        .max()
        .expect("owner strings")
        .max(selector_len);
    (
        Usage {
            items: 1 + strings.len() + 2 + 1,
            bytes: std::mem::size_of::<Selection>()
                + strings.iter().map(|value| value.len()).sum::<usize>()
                + package.root().as_os_str().len()
                + package.manifest().as_os_str().len()
                + selector_len,
        },
        maximum,
    )
}

#[test]
fn actual_owned_copies_fit_exact_prior_usage_and_record_each_one_over_target() {
    for package in [
        path("owner", "1.2.3", "/work/owner"),
        git("π", "1.2.3-rc.1+build", "/work/根"),
    ] {
        let prior = Usage {
            items: 3,
            bytes: 19,
        };
        let (retained, string_limit) = retained(&package);
        let exact = Usage {
            items: prior.items + retained.items,
            bytes: prior.bytes + retained.bytes,
        };
        // The exact account records nothing; one less on each axis is an
        // exceedance of exactly that axis whose high water is the exact figure,
        // and every owned copy is still produced on the original account.
        for (items, bytes, strings, exceeded) in [
            (exact.items, exact.bytes, string_limit, None),
            (
                exact.items - 1,
                exact.bytes,
                string_limit,
                Some((INPUT_ITEMS_RESOURCE, exact.items)),
            ),
            (
                exact.items,
                exact.bytes - 1,
                string_limit,
                Some((INPUT_BYTES_RESOURCE, exact.bytes)),
            ),
            (
                exact.items,
                exact.bytes,
                string_limit - 1,
                Some((INPUT_STRING_RESOURCE, string_limit)),
            ),
        ] {
            let mut allocation = Allocation::with_limits(items, bytes, strings);
            allocation.items(prior.items).expect("prior items");
            allocation.bytes(prior.bytes).expect("prior bytes");
            OWNED_BUFFERS.with(|owned| owned.set(0));
            let result = Selection::derive(
                toolchain::supported_cargo_commit(),
                &package,
                package.manifest(),
                &mut allocation,
            );
            assert!(result
                .expect("accounting targets never refuse")
                .matches_package(&package));
            assert_eq!(allocation.usage(), exact);
            assert_eq!(OWNED_BUFFERS.with(std::cell::Cell::get), retained.items - 1);
            let record = allocation.exceedances();
            match exceeded {
                None => assert!(!record.is_exceeded()),
                Some((resource, high_water)) => {
                    let axis = record
                        .axis(resource)
                        .expect("the one-less axis is recorded");
                    assert_eq!(axis.target, (high_water - 1) as u64);
                    assert_eq!(axis.high_water, high_water as u64);
                    assert_eq!(record.axes().count(), 1);
                }
            }
            allocation
                .items(0)
                .expect("an exceedance never latches the account");
        }
    }
}
