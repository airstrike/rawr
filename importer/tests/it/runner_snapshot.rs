use crate::p3;

#[test]
fn admitted_generation_compiles_and_evaluates_the_exact_snapshot() {
    let project = rawr_import::store::TestProject::new("rawr-runner-snapshot");
    let root = project.path().to_path_buf();
    let cache = rawr_import::cache::Writer::new(&root).expect("the cache binds");
    let process = rawr_import::process::Owner::new(&root).expect("the process owner binds");
    let mut supervisor = rawr_import::supervisor::Owner::new();

    let (request, snapshot) = p3::compiled_request_at(&root, b"let answer = 40 + 2; answer");
    let generated = request
        .generate(&rawr_import::cargo::Limits::standard())
        .expect("the compiling runner generates");
    let lane = crate::shared::lane();
    let cargo = rawr_import::cargo::Owner::acquire().expect("the Cargo lane is free");
    let built = generated
        .build(&cargo, &cache)
        .expect("the compiling runner builds");
    drop(cargo);
    drop(lane);
    let admitted = supervisor
        .bootstrap(built, snapshot, &process)
        .expect("the exact snapshot compiles and evaluates before admission");
    admitted.shutdown().expect("the admitted runner reaps");

    let (request, snapshot) = p3::compiled_request_at(&root, b"throw \"evaluated snapshot\"");
    let generated = request
        .generate(&rawr_import::cargo::Limits::standard())
        .expect("the rejecting runner generates");
    let lane = crate::shared::lane();
    let cargo = rawr_import::cargo::Owner::acquire().expect("the Cargo lane is free");
    let built = generated
        .build(&cargo, &cache)
        .expect("the identical compiling runner is cached");
    drop(cargo);
    drop(lane);
    assert!(supervisor.bootstrap(built, snapshot, &process).is_err());
}
