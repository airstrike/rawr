use super::{Attempt, Axis, Native, Receipt, Refusal};
use crate::{host, host::ImportAllocation as _, package::import::runner, Engine};
use rawr_import::{runner::manifest::Carrier, surface::identity};
use std::sync::{Arc, Barrier};

/// Versioned empty metadata fixture, never a live native admission.
pub(crate) fn carrier() -> Carrier {
    carrier_entries(&[])
}

fn carrier_entries(entries: &[([u8; 32], [u8; 32], u32)]) -> Carrier {
    Carrier::transform(&carrier_bytes(entries)).unwrap()
}

pub(crate) fn carrier_bytes(entries: &[([u8; 32], [u8; 32], u32)]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RAWRGM01");
    bytes.extend_from_slice(&[3; 32]);
    bytes.extend_from_slice(&0_u64.to_le_bytes());
    bytes.extend_from_slice(&(entries.len() as u64).to_le_bytes());
    for (identity, signature, arguments) in entries {
        bytes.extend_from_slice(identity);
        bytes.extend_from_slice(signature);
        bytes.extend_from_slice(&arguments.to_le_bytes());
    }
    bytes.extend_from_slice(&20_u64.to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u64.to_le_bytes());
    bytes.extend_from_slice(&0_u64.to_le_bytes());
    bytes.extend_from_slice(&0_u64.to_le_bytes());
    // Exact current manifest, wire, generator, runtime and profile versions.
    for version in rawr_import::runner::manifest::carrier_versions() {
        bytes.extend_from_slice(&version.to_le_bytes());
    }
    bytes
}

#[test]
fn reconstructed_native_shares_the_latch_without_a_baseline_chain() {
    let native = Native::decode(&carrier_bytes(&[])).unwrap();
    let baseline = Arc::downgrade(&native.inner);
    let receipt = native.attempt().unwrap().finish();
    let engine = Engine::new().retain_imported_native(native).unwrap();
    let replacement = engine
        .reconstruct_imported_native(&carrier_bytes(&[]))
        .unwrap();
    drop(engine);
    // The live receipt keeps the original metadata; the replacement doesn't.
    assert!(baseline.upgrade().is_some());
    drop(receipt);
    assert!(baseline.upgrade().is_none());
    let replacement_baseline = Arc::downgrade(&replacement.inner);
    let engine = Engine::new().retain_imported_native(replacement).unwrap();
    let next = engine
        .reconstruct_imported_native(&carrier_bytes(&[]))
        .unwrap();
    drop(engine);
    assert!(replacement_baseline.upgrade().is_none());
    // A refusal in the newest compiler latches the whole chain's account.
    let mut attempt = next.attempt().unwrap();
    assert!(attempt.retain_checked(None, Some(1)).is_err());
    assert_eq!(next.first_refusal(), Some(Refusal { axis: Axis::Items }));
}

#[test]
fn reconstructed_native_preserves_decode_error_and_requires_original_ownership() {
    let engine = Engine::new();
    assert!(matches!(
        engine.reconstruct_imported_native(&carrier_bytes(&[])),
        Err(runner::Error::NativeOwner)
    ));

    let native = Native::transform(carrier());
    let engine = engine.retain_imported_native(native).unwrap();
    assert!(matches!(
        engine.reconstruct_imported_native(b"invalid manifest"),
        Err(runner::Error::Manifest(_))
    ));
    assert_eq!(
        engine.imported_native.as_ref().unwrap().first_refusal(),
        None
    );
}

#[test]
fn native_refusal_names_the_first_unrepresentable_axis() {
    let cases = [
        (None, None, Axis::Items),
        (None, Some(1), Axis::Items),
        (Some(usize::MAX), None, Axis::Bytes),
    ];
    for (items, bytes, axis) in cases {
        let native = Native::transform(carrier());
        let mut attempt = native.attempt().unwrap();
        assert!(matches!(
            attempt.retain_checked(items, bytes),
            Err(rawr_import::surface::Error::Allocation)
        ));
        assert_eq!(native.first_refusal(), Some(Refusal { axis }));
    }
    // Any representable amount is accepted: nothing is counted against it.
    let native = Native::transform(carrier());
    let mut attempt = native.attempt().unwrap();
    for _ in 0..3 {
        attempt
            .retain_checked(Some(usize::MAX), Some(usize::MAX))
            .unwrap();
    }
    assert_eq!(native.first_refusal(), None);
}

#[test]
fn native_account_baseline_survives_native_until_last_receipt() {
    let native = Native::transform(carrier());
    let attempt = native.attempt().unwrap();
    let baseline = Arc::downgrade(&native.inner);
    let receipt = attempt.finish();
    assert_eq!(
        std::mem::size_of::<Receipt>(),
        std::mem::size_of::<Attempt>()
    );
    drop(native);
    assert!(baseline.upgrade().is_some());
    drop(receipt);
    assert!(baseline.upgrade().is_none());
}

#[test]
fn native_account_simultaneous_scopes_share_one_latch() {
    let native = Native::transform(carrier());
    let baseline = native.owners();
    let barrier = Barrier::new(3);
    std::thread::scope(|scope| {
        let spawn = |refuse: bool| {
            let (native, barrier) = (&native, &barrier);
            scope.spawn(move || {
                let attempt = native.attempt();
                barrier.wait();
                if refuse {
                    let mut attempt = attempt.unwrap();
                    assert!(attempt.retain_checked(Some(1), None).is_err());
                    barrier.wait();
                    true
                } else {
                    barrier.wait();
                    let mut attempt = attempt.unwrap();
                    attempt.retain_checked(Some(1), Some(1)).is_err()
                }
            })
        };
        let first = spawn(true);
        let second = spawn(false);
        barrier.wait();
        // Both scopes hold the one decoded baseline.
        assert_eq!(native.owners(), baseline + 2);
        barrier.wait();
        assert!(first.join().unwrap());
        assert!(second.join().unwrap(), "the latch refuses the other scope");
    });
    assert_eq!(native.owners(), baseline);
    assert_eq!(native.first_refusal(), Some(Refusal { axis: Axis::Bytes }));
}

fn plan() -> Box<[u8]> {
    rawr_import::test_fixture::runner_native_facade_plan()
}

#[test]
fn retained_install_authenticates_structural_rows() {
    let carrier = carrier_entries(&[([31; 32], [32; 32], 0)]);
    let mut native = Native::transform(carrier);
    let mut engine = Engine::new();
    let definitions = runner::install_retained(&mut engine, &plan(), &mut native).unwrap();
    let row = runner::StructuralRow::new(
        "native",
        "",
        identity::Selector::StructConstructor(identity::Form::Unit).fingerprint(),
        [31; 32],
        [32; 32],
        0,
        runner::StructuralKind::Constructor,
    );
    runner::admit_generated_structural_retained(&mut engine, &definitions, &[row], &mut native)
        .unwrap();
    let bindings =
        runner::authenticate_generated_structural_retained(&definitions, &[row], &mut native)
            .unwrap();
    assert_eq!(bindings.len(), 1);
}

fn runtime_fixture() -> (
    Engine,
    Box<[host::facade::Definition]>,
    runner::StructuralRow,
) {
    let carrier = carrier_entries(&[([31; 32], [32; 32], 0)]);
    let mut native = Native::transform(carrier);
    let mut engine = Engine::new();
    let definitions = runner::install_retained(&mut engine, &plan(), &mut native).unwrap();
    engine.imported_native = Some(native);
    (
        engine,
        definitions,
        runner::StructuralRow::new(
            "native",
            "",
            identity::Selector::StructConstructor(identity::Form::Unit).fingerprint(),
            [31; 32],
            [32; 32],
            0,
            runner::StructuralKind::Constructor,
        ),
    )
}

fn bind_runtime(
    scope: &runner::Runtime<'_>,
    definitions: &[host::facade::Definition],
) -> host::runtime::Registry {
    let mut registry = host::runtime::Registry::new();
    for definition in definitions {
        let mut builder = host::runtime::Builder::new(definition.clone());
        for adapter in scope
            .structural()
            .iter()
            .filter(|adapter| adapter.root() == definition.root())
        {
            builder
                .bind_imported_structural(adapter, |_, _| Ok(crate::Dynamic::UNIT))
                .unwrap();
        }
        registry.install(builder.finish()).unwrap();
    }
    registry
}

#[test]
fn runtime_scope_binds_authenticated_rows_and_refuses_a_duplicate_binding() {
    let (engine, definitions, structural) = runtime_fixture();
    let mut live = engine.begin_imported_runtime().unwrap();
    live.authenticate_structural(&definitions, &[structural])
        .unwrap();
    let registry = bind_runtime(&live, &definitions);
    live.finish(registry).unwrap();

    let mut scope = engine.begin_imported_runtime().unwrap();
    scope
        .authenticate_structural(&definitions, &[structural])
        .unwrap();
    let source = definitions
        .iter()
        .find(|definition| definition.root() == "native")
        .unwrap();
    let mut builder = host::runtime::Builder::new(source.clone());
    let adapter = &scope.structural()[0];
    builder
        .bind_imported_structural(adapter, |_, _| Ok(crate::Dynamic::UNIT))
        .unwrap();
    assert!(matches!(
        builder.bind_imported_structural(adapter, |_, _| Ok(crate::Dynamic::UNIT)),
        Err(host::runtime::Error::DuplicateStructuralBinding { .. })
    ));
}

#[test]
fn native_account_runtime_cannot_finish_incomplete_authentication() {
    let mut engine = Engine::new();
    engine.imported_native = Some(Native::transform(carrier()));
    let native = engine.imported_native.as_ref().unwrap();
    let before = native.owners();
    let scope = engine.begin_imported_runtime().unwrap();
    assert!(scope.finish(host::runtime::Registry::new()).is_err());
    assert_eq!(native.owners(), before);
}

#[test]
fn structural_row_outside_the_manifest_panics() {
    let (engine, definitions, structural) = runtime_fixture();
    let native = engine.imported_native.as_ref().unwrap();
    let mut attempt = native.attempt().unwrap();
    // A row the empty carrier lacks is a rawr bug, so it panics.
    let missing = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ =
            runner::authenticate_structural(&definitions, &carrier(), &[structural], &mut attempt);
    }))
    .expect_err("a structural row outside the manifest panics");
    assert_eq!(
        missing.downcast_ref::<&str>(),
        Some(&"each structural adapter is in the runner's manifest")
    );
}

/// A checked-arithmetic refusal is terminal for the shared account. A
/// later charge on any scope, a fresh attempt and a reconstruction sharing
/// the account are all refused, and the first cause stays; cleanup (dropping
/// scopes) still releases the decoded metadata.
#[test]
fn arithmetic_refusal_suppresses_every_later_admission_and_keeps_the_first_cause() {
    let native = Native::transform(carrier());
    let baseline = native.owners();
    let mut first = native.attempt().unwrap();
    let mut second = native.attempt().unwrap();
    assert_eq!(native.first_refusal(), None);
    // A genuine cause: the producer could not represent the item amount.
    assert!(matches!(
        first.retain_checked(None, Some(1)),
        Err(rawr_import::surface::Error::Allocation)
    ));
    let cause = Refusal { axis: Axis::Items };
    assert_eq!(native.first_refusal(), Some(cause));
    // Representable charges are refused on both scopes.
    for attempt in [&mut first, &mut second] {
        assert!(matches!(
            attempt.retain_checked(Some(1), Some(1)),
            Err(rawr_import::surface::Error::Allocation)
        ));
    }
    // A later arithmetic failure on another axis does not replace the cause.
    assert!(matches!(
        second.retain_checked(Some(0), None),
        Err(rawr_import::surface::Error::Allocation)
    ));
    // A fresh attempt and a reconstruction sharing the account are refused.
    assert!(matches!(
        native.attempt(),
        Err(rawr_import::surface::Error::Allocation)
    ));
    assert!(matches!(
        native.reconstruct(&carrier_bytes(&[])),
        Err(runner::Error::Surface(
            rawr_import::surface::Error::Allocation
        ))
    ));
    assert_eq!(native.first_refusal(), Some(cause), "the first cause stays");
    // Cleanup only: dropping the scopes still releases the metadata they held.
    drop(first);
    drop(second);
    assert_eq!(native.owners(), baseline);
    assert_eq!(native.first_refusal(), Some(cause));
}

#[test]
fn native_account_runtime_scope_types_only_arithmetic_refusal() {
    let mut engine = Engine::new();
    engine.imported_native = Some(Native::transform(carrier()));
    let native = engine.imported_native.as_ref().unwrap();
    let before = native.owners();
    let scope = engine.begin_imported_runtime().unwrap();
    assert_eq!(native.owners(), before + 1);
    drop(scope);
    assert_eq!(native.owners(), before);

    // Once a producer's arithmetic refuses, a runtime scope is refused, typed.
    let mut attempt = native.attempt().unwrap();
    assert!(attempt.retain_checked(Some(1), None).is_err());
    drop(attempt);
    assert!(matches!(
        engine.begin_imported_runtime(),
        Err(host::runtime::Error::ImportedAllocation)
    ));
    assert_eq!(native.owners(), before);
    assert_eq!(native.first_refusal(), Some(Refusal { axis: Axis::Bytes }));
}
