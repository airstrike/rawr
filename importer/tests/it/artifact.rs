use crate::p3;

use std::fs;

#[test]
fn supervisor_artifact_proof_detects_post_link_tampering() {
    let _turn = rawr_import::store::test_turn();
    let project = rawr_import::store::TestProject::new("rawr-p3-artifact");
    let root = project.path();
    let generated = p3::generated_at(root);
    let wrong_key = p3::generated_at(root);
    let missing = p3::generated_at(root);
    let wrong_record = p3::generated_at(root);
    let cache = rawr_import::cache::Writer::new(root).expect("the fixed cache writer binds");
    let cargo = rawr_import::cargo::Owner::acquire().expect("the sole Cargo owner is available");
    let built = generated
        .build(&cargo, &cache)
        .expect("the generated runner builds and seals");
    built
        .artifact()
        .revalidate()
        .expect("the untouched executable revalidates");

    let executable = fs::read_dir(cache.root())
        .expect("the cache root is readable")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.is_dir())
        .expect("one built entry exists");
    let carrier_path = executable.join("rawr-generated-manifest.carrier");
    let carrier = fs::read(&carrier_path).expect("the carrier is readable");
    let mut tampered = carrier.clone();
    tampered[8] ^= 1;
    fs::write(&carrier_path, tampered).expect("the disposable carrier can be tampered");
    let warm = wrong_key.build(&cargo, &cache);
    assert!(matches!(
        warm,
        Err(rawr_import::runner::BuildError::Manifest(
            rawr_import::runner::manifest::Error::Carrier
        ))
    ));
    fs::write(&carrier_path, &carrier).expect("the exact carrier is restored");

    fs::remove_file(&carrier_path).expect("the disposable carrier can be removed");
    assert!(matches!(
        missing.build(&cargo, &cache),
        Err(rawr_import::runner::BuildError::Io { .. })
    ));
    fs::write(&carrier_path, &carrier).expect("the exact carrier is restored");

    let mut tampered = carrier.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 1;
    fs::write(&carrier_path, tampered).expect("one derived record can be tampered");
    assert!(matches!(
        wrong_record.build(&cargo, &cache),
        Err(rawr_import::runner::BuildError::Manifest(
            rawr_import::runner::manifest::Error::Carrier
        ))
    ));
    fs::write(&carrier_path, carrier).expect("the exact carrier is restored again");

    let executable = executable.join("target/debug/rawr-generated-runner");
    let mut bytes = fs::read(&executable).expect("the built executable is readable");
    bytes[0] ^= 1;
    fs::write(&executable, bytes).expect("the disposable cache executable can be tampered");
    assert!(built.artifact().revalidate().is_err());
    drop(built);
    drop(cargo);
}
