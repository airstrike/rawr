use std::{
    env,
    fs::File,
    io::{Read, Write},
};

fn main() {
    // Tell Cargo that if the given environment variable changes, to rerun this build script.
    // Naming any path stops Cargo rerunning this script when it changes
    // itself, so it names itself too.
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build.template");
    println!("cargo:rerun-if-env-changed=RAWR_AHASH_SEED");
    println!("cargo:rerun-if-env-changed=RAWR_HASHING_SEED");
    parser_digest();
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

/// The sources a parse and its cell encoding come from. A parse cell
/// carries their digest, so a `rawr` built from other sources never decodes
/// it (`src/package/cell.rs`).
const PARSER_SOURCES: &[&str] = &[
    "src/ast",
    "src/item",
    "src/item.rs",
    "src/package/cell.rs",
    "src/parse.rs",
    "src/parser",
    "src/parser.rs",
    "src/preview.rs",
    "src/project/frontmatter.rs",
    "src/source.rs",
    "src/tokenizer.rs",
    "src/types/position.rs",
];

/// FNV-1a over each parser source's path and bytes, in path order.
fn parser_digest() {
    fn collect(path: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
        if path.is_dir() {
            for entry in std::fs::read_dir(path).expect("cannot read a parser source directory") {
                collect(&entry.expect("cannot read a parser source").path(), files);
            }
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path.to_path_buf());
        }
    }
    let mut files = Vec::new();
    for source in PARSER_SOURCES {
        println!("cargo:rerun-if-changed={source}");
        collect(std::path::Path::new(source), &mut files);
    }
    files.sort();
    let mut digest = 0xcbf2_9ce4_8422_2325_u64;
    for file in files {
        let bytes = std::fs::read(&file).expect("cannot read a parser source");
        for byte in file.to_string_lossy().bytes().chain(bytes) {
            digest = (digest ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    println!("cargo:rustc-env=RAWR_PARSER_DIGEST={digest:016x}");
}
