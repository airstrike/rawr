use crate::p2;

use rawr_import::classify::Policy;
use rawr_import::surface::{Origin, Snapshot};

#[test]
fn demand_is_snapshot_bound_but_surface_is_position_and_multiplicity_neutral() {
    let _turn = rawr_import::store::test_turn();
    let limits = rawr_import::cargo::Limits::standard();
    let contract = p2::graph()
        .classify(&Policy::standard(), &limits)
        .expect("the real extracted graph classifies");
    let identity = p2::outcome(&contract, "facade_a", &["integer_only"])
        .identity()
        .clone();
    let first_snapshot = snapshot("/app/Cargo.toml", b"integer_only()", &limits);
    let second_snapshot = snapshot("/app/Cargo.toml", b"integer_only();", &limits);
    assert_ne!(first_snapshot, second_snapshot);

    let first_origin =
        Origin::checked("/app/main.rs", 0, 12, &limits).expect("the first use position is bounded");
    let moved_origin =
        Origin::checked("/app/main.rs", 1, 13, &limits).expect("the moved use position is bounded");
    let first = demand(
        &contract,
        first_snapshot,
        identity.clone(),
        first_origin.clone(),
        false,
        &limits,
    );
    let duplicate = demand(
        &contract,
        first_snapshot,
        identity.clone(),
        first_origin,
        true,
        &limits,
    );
    let moved = demand(
        &contract,
        first_snapshot,
        identity.clone(),
        moved_origin.clone(),
        false,
        &limits,
    );
    let changed_body = demand(
        &contract,
        second_snapshot,
        identity,
        moved_origin,
        false,
        &limits,
    );

    assert_eq!(first.entries().len(), 1);
    assert_eq!(duplicate.entries().len(), 1);
    assert_eq!(first.fingerprint(), duplicate.fingerprint());
    assert_eq!(first.canonical_bytes(), duplicate.canonical_bytes());
    assert_eq!(first.fingerprint(), moved.fingerprint());
    assert_ne!(first.canonical_bytes(), moved.canonical_bytes());
    assert_eq!(first.fingerprint(), changed_body.fingerprint());
    assert_ne!(moved.canonical_bytes(), changed_body.canonical_bytes());
}

#[test]
fn snapshot_identity_includes_the_exact_entry_target() {
    let _turn = rawr_import::store::test_turn();
    let limits = rawr_import::cargo::Limits::standard();
    assert_ne!(
        snapshot("/app/first.rs", b"same", &limits),
        snapshot("/app/second.rs", b"same", &limits)
    );
}

fn snapshot(entry: &str, body: &[u8], limits: &rawr_import::cargo::Limits) -> Snapshot {
    let mut builder = Snapshot::builder(limits);
    builder.entry(entry).expect("the entry target is bounded");
    builder
        .file("/app/main.rs", body)
        .expect("the exact path bytes and contents are bounded");
    builder.seal().expect("the nonempty snapshot seals")
}

fn demand(
    contract: &rawr_import::Contract<rawr_import::Classified>,
    snapshot: Snapshot,
    identity: rawr_import::surface::identity::Identity,
    origin: Origin,
    duplicate: bool,
    limits: &rawr_import::cargo::Limits,
) -> rawr_import::Demand<rawr_import::Sealed, ()> {
    let mut compiler = contract
        .compiler((), snapshot, limits)
        .expect("the exact contract allocation transfers");
    compiler
        .require(identity.clone(), origin.clone())
        .expect("the exact use is demandable");
    if duplicate {
        compiler
            .require(identity, origin)
            .expect("the duplicate use remains bounded");
    }
    compiler.seal().expect("the symbolic demand seals")
}
