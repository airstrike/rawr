use std::{
    env,
    fs::File,
    io::{Read, Write},
};

fn main() {
    // Tell Cargo that if the given environment variable changes, to rerun this build script.
    println!("cargo:rerun-if-changed=build.template");
    println!("cargo:rerun-if-env-changed=RAWR_AHASH_SEED");
    println!("cargo:rerun-if-env-changed=RAWR_HASHING_SEED");
    let mut contents = String::new();

    File::open("build.template")
        .expect("cannot open `build.template`")
        .read_to_string(&mut contents)
        .expect("cannot read from `build.template`");

    let seed = env::var("RAWR_HASHING_SEED")
        .or_else(|_| env::var("RAWR_AHASH_SEED"))
        .map_or_else(|_| "None".into(), |s| format!("Some({s})"));

    contents = contents.replace("{{HASHING_SEED}}", &seed);

    // Rewriting identical bytes would still signal a source change to every
    // watcher of this crate, such as a running Rawr application.
    let target = "src/config/hashing_env.rs";
    if std::fs::read(target).is_ok_and(|current| current == contents.as_bytes()) {
        return;
    }
    File::create(target)
        .expect("cannot create `hashing_env.rs`")
        .write_all(contents.as_bytes())
        .expect("cannot write to `config/hashing_env.rs`");
}
