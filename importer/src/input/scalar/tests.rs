use std::fmt::Write as _;
use std::num::NonZeroUsize;
use std::path::Path;

use super::Value;
use crate::cargo::config::{projection, Environment};
use crate::cargo::Limits;
use crate::input::proof::{Recipe, Role};
use crate::input::{Allocation, Usage};
use crate::process::parallelism;
use crate::{Error, Result};

fn projection() -> projection::Projection {
    let parallelism = parallelism::Proof::fixture(NonZeroUsize::new(3).expect("nonzero jobs"))
        .expect("fixture parallelism seals");
    projection::Projection::derive_governed(
        &Environment::from_pairs([("RUSTFLAGS", ""), ("HOME", "/fixture/home")])
            .expect("fixture environment seals"),
        "aarch64-apple-darwin",
        "dev",
        parallelism,
        &Limits::standard(),
    )
    .expect("governed projection seals")
}

fn append(values: &mut Vec<Recipe>, value: Value<'_>, allocation: &mut Allocation) -> Result<()> {
    allocation.items(1)?;
    values.try_reserve(1).expect("fixture recipe slot reserves");
    values.push(value.retain(allocation)?);
    Ok(())
}

fn keys(values: &[Recipe]) -> Vec<(&str, Option<&str>, &str)> {
    values
        .iter()
        .map(|value| match value {
            Recipe::Fact {
                package_id,
                name,
                value,
                ..
            } => (name.as_ref(), package_id.as_deref(), value.as_ref()),
            Recipe::EnvironmentProjection { key, value, .. } => {
                (key.as_ref(), None, value.as_deref().unwrap_or("<absent>"))
            }
            _ => panic!("only scalar fixture recipes are retained"),
        })
        .collect()
}

#[test]
fn projection_emissions_borrow_exact_entries_before_the_provenance_fact() {
    let projection = projection();
    let mut expected = projection.entries();
    let mut allocation = Allocation::for_proof(
        Usage {
            items: 7,
            bytes: 23,
        },
        &Limits::standard(),
    );
    let provenance = [0xab; 32];
    let expected_hex = "ab".repeat(32);
    let mut provenance_seen = false;
    super::config(
        &projection,
        &provenance,
        &Limits::standard(),
        &mut allocation,
        |value, _| {
            match value {
                Value::EnvironmentProjection { schema, key, value } => {
                    let (expected_key, expected_value) =
                        expected.next().expect("one current entry");
                    assert_eq!(schema, projection.schema());
                    assert_eq!((key, value), (expected_key, expected_value));
                    assert_eq!(key.as_ptr(), expected_key.as_ptr());
                    assert_eq!(value.map(str::as_ptr), expected_value.map(str::as_ptr));
                    assert!(!provenance_seen);
                }
                Value::Fact {
                    role,
                    package_id,
                    name,
                    value,
                } => {
                    assert_eq!(role, Role::Config);
                    assert_eq!(package_id, None);
                    assert_eq!(name, "config-provenance");
                    assert_eq!(value, expected_hex);
                    assert!(expected.next().is_none());
                    provenance_seen = true;
                }
            }
            Ok(())
        },
    )
    .expect("borrowed observations emit");
    assert!(provenance_seen);
    assert_eq!(
        allocation.usage(),
        Usage {
            items: 8,
            bytes: 87
        }
    );
}

#[test]
fn config_retention_matches_original_order_and_partial_account_admission() {
    let projection = projection();
    let provenance = [0x3f; 32];
    let limits = Limits::standard();
    let prior = Usage {
        items: 7,
        bytes: 23,
    };
    let mut measured = Allocation::for_proof(prior, &limits);
    let mut values = Vec::new();
    super::config(
        &projection,
        &provenance,
        &limits,
        &mut measured,
        |value, allocation| append(&mut values, value, allocation),
    )
    .expect("config measurement succeeds");
    let exact = measured.usage();
    for (items, bytes) in [
        (exact.items, exact.bytes),
        (exact.items - 1, exact.bytes),
        (exact.items, exact.bytes - 1),
        (prior.items + 2, exact.bytes),
    ] {
        let mut limits = limits;
        limits.items = items;
        limits.input_proof_bytes = bytes;
        let mut actual_account = Allocation::for_proof(prior, &limits);
        let mut actual = Vec::new();
        let result = super::config(
            &projection,
            &provenance,
            &limits,
            &mut actual_account,
            |value, allocation| append(&mut actual, value, allocation),
        );
        let mut reference_account = Allocation::for_proof(prior, &limits);
        let mut reference = Vec::new();
        let expected = original_config(
            &projection,
            &provenance,
            &limits,
            &mut reference_account,
            &mut reference,
        );
        assert_eq!(result.is_ok(), expected.is_ok());
        assert_eq!(actual_account.usage(), reference_account.usage());
        assert_eq!(actual_account.refused, reference_account.refused);
        assert_eq!(keys(&actual), keys(&reference));
        if result.is_err() {
            let used = actual_account.usage();
            assert!(matches!(
                actual_account.bytes(0),
                Err(Error::InputAccountRefused)
            ));
            assert_eq!(actual_account.usage(), used);
        }
    }
}

fn original_config(
    projection: &projection::Projection,
    provenance: &[u8; 32],
    limits: &Limits,
    allocation: &mut Allocation,
    recipes: &mut Vec<Recipe>,
) -> Result<()> {
    for (key, value) in projection.entries() {
        allocation.exceeded(
            "Cargo environment projection key",
            limits.string_bytes,
            key.len(),
        );
        if let Some(value) = value {
            allocation.exceeded(
                "Cargo environment projection value",
                limits.string_bytes,
                value.len(),
            );
        }
        allocation.items(1)?;
        recipes
            .try_reserve(1)
            .expect("reference recipe slot reserves");
        recipes.push(Recipe::projection(
            projection.schema(),
            key,
            value,
            allocation,
        )?);
    }
    allocation.items(1)?;
    allocation.bytes(64)?;
    let mut text = String::new();
    text.try_reserve_exact(64)
        .expect("reference provenance reserves");
    for byte in provenance {
        write!(text, "{byte:02x}").expect("reference provenance formats");
    }
    allocation.items(1)?;
    recipes
        .try_reserve(1)
        .expect("reference fact slot reserves");
    recipes.push(Recipe::fact(
        Role::Config,
        "config-provenance",
        &text,
        allocation,
    )?);
    Ok(())
}

#[test]
fn selection_borrows_spellings_and_stops_before_profile_after_callback_error() {
    let target = String::from("aarch64-apple-darwin");
    let mut allocation = Allocation::for_proof(
        Usage {
            items: 7,
            bytes: 23,
        },
        &Limits::standard(),
    );
    let mut count = 0;
    let result = super::selection(&target, "custom-profile", &mut allocation, |value, _| {
        count += 1;
        assert!(
            matches!(value, Value::Fact { role: Role::Target, package_id: None,
            name: "target", value } if value.as_ptr() == target.as_ptr())
        );
        Err(Error::ResolutionDrift {
            command: "scalar test",
            field: "target",
        })
    });
    assert!(matches!(
        result,
        Err(Error::ResolutionDrift {
            field: "target",
            ..
        })
    ));
    assert_eq!(count, 1);
    assert_eq!(
        allocation.usage(),
        Usage {
            items: 7,
            bytes: 23
        }
    );
}

#[test]
fn selection_and_package_keep_intervening_input_order_and_original_charges() {
    let limits = Limits::standard();
    let prior = Usage {
        items: 7,
        bytes: 23,
    };
    let id = "path+file:///fixture/pkg#pkg@1.2.3";
    let root = Path::new("/fixture/pkg");
    let mut account = Allocation::for_proof(prior, &limits);
    let mut values = Vec::new();
    super::selection(
        "target-spelling",
        "custom-profile",
        &mut account,
        |value, account| append(&mut values, value, account),
    )
    .expect("selection emits");
    append(
        &mut values,
        Value::Fact {
            role: Role::PackageManifest,
            package_id: Some(id),
            name: "fixture-manifest-marker",
            value: "observed",
        },
        &mut account,
    )
    .expect("intervening fixture marker retains");
    super::package(id, root, &mut account, |value, account| {
        append(&mut values, value, account)
    })
    .expect("package emits");
    let mut expected = Allocation::for_proof(prior, &limits);
    let mut reference = Vec::new();
    for (role, name, value) in [
        (Role::Target, "target", "target-spelling"),
        (Role::Profile, "profile", "custom-profile"),
    ] {
        expected.items(1).expect("reference slot admits");
        reference
            .push(Recipe::fact(role, name, value, &mut expected).expect("reference fact retains"));
    }
    expected.items(1).expect("reference marker slot admits");
    reference.push(
        Recipe::package_fact(
            Role::PackageManifest,
            id,
            "fixture-manifest-marker",
            "observed",
            &mut expected,
        )
        .expect("reference marker retains"),
    );
    let name = expected
        .formatted(format_args!("package:{id}"))
        .expect("package name admits");
    expected.path(root).expect("package path admits");
    expected.items(1).expect("reference package slot admits");
    reference.push(
        Recipe::package_fact(
            Role::DependencySource,
            id,
            &name,
            "/fixture/pkg",
            &mut expected,
        )
        .expect("reference package fact retains"),
    );
    assert_eq!(keys(&values), keys(&reference));
    assert_eq!(account.usage(), expected.usage());
    assert_eq!(
        keys(&values)
            .iter()
            .map(|value| value.0)
            .collect::<Vec<_>>(),
        [
            "target",
            "profile",
            "fixture-manifest-marker",
            name.as_str()
        ]
    );
}

#[test]
fn package_exceedance_preserves_format_and_path_charges_before_callback() {
    let mut limits = Limits::standard();
    let id = "pkg";
    let root = Path::new("/root");
    let prior = Usage {
        items: 7,
        bytes: 23,
    };
    let exact = Usage {
        items: prior.items + 2,
        bytes: prior.bytes + "package:pkg".len() + root.as_os_str().len(),
    };
    limits.input_proof_bytes = exact.bytes - 1;
    let mut allocation = Allocation::for_proof(prior, &limits);
    let mut called = false;
    // The formatted name and the root path are charged before the
    // callback; the one-less byte target is recorded as an exceedance by the
    // path charge, already visible to the callback, and the emission proceeds.
    super::package(id, root, &mut allocation, |_, allocation| {
        called = true;
        let bytes = allocation
            .exceedances()
            .axis(crate::input::INPUT_BYTES_RESOURCE)
            .copied()
            .expect("the path charge recorded the exceedance before the callback");
        assert_eq!(bytes.target, (exact.bytes - 1) as u64);
        assert_eq!(bytes.high_water, exact.bytes as u64);
        Ok(())
    })
    .expect("accounting targets never refuse");
    assert!(called);
    assert_eq!(allocation.usage(), exact);
    assert!(allocation
        .exceedances()
        .axis(crate::input::INPUT_ITEMS_RESOURCE)
        .is_none());
    allocation
        .items(0)
        .expect("an exceedance never latches the account");
    // Only a genuine failure latches; a latched account emits nothing further.
    called = false;
    allocation.refuse();
    assert!(matches!(
        super::selection("target", "profile", &mut allocation, |_, _| {
            called = true;
            Ok(())
        }),
        Err(Error::InputAccountRefused)
    ));
    assert!(!called);
    assert_eq!(allocation.usage(), exact);
}
