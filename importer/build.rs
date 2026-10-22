use std::env;
use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest as _, Sha256};

const RUSTC_COMMIT: &str = "375b1431b7d89d1c2e2bc168c011848ae12b7d14";

/// The Rawr packages a generated harness builds through `rawr-iced`.
const HOST_PACKAGES: [&str; 10] = [
    ".",
    "canonical",
    "codegen",
    "directory",
    "iced",
    "importer",
    "location",
    "toml",
    "watch",
    "watch/notify",
];

fn main() -> Result<(), Box<dyn Error>> {
    // Naming any path stops Cargo rerunning this script when it changes
    // itself, so it names itself too.
    println!("cargo:rerun-if-changed=build.rs");
    if env::var_os("CARGO_FEATURE_EMBEDDED_HOST").is_some() {
        embed_host()?;
    }
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_RUSTC_DRIVER");
    println!("cargo:rerun-if-env-changed=RUSTC");
    if env::var_os("CARGO_FEATURE_RUSTC_DRIVER").is_none() {
        return Ok(());
    }

    let rustc = env::var_os("RUSTC").ok_or("Cargo did not provide RUSTC")?;
    let version = Command::new(&rustc).arg("-Vv").output()?;
    if !version.status.success() {
        return Err("the pinned rustc version query failed".into());
    }
    let version = String::from_utf8(version.stdout)?;
    let expected = format!("commit-hash: {RUSTC_COMMIT}");
    if !version.lines().any(|line| line == expected) {
        return Err(format!("rustc-driver requires pinned {expected}").into());
    }

    let sysroot = Command::new(&rustc).args(["--print", "sysroot"]).output()?;
    if !sysroot.status.success() {
        return Err("the pinned rustc sysroot query failed".into());
    }
    let sysroot = PathBuf::from(String::from_utf8(sysroot.stdout)?.trim());
    let library = sysroot.join("lib").canonicalize()?;
    let target_library = sysroot
        .join("lib/rustlib/aarch64-apple-darwin/lib")
        .canonicalize()?;
    for path in [&library, &target_library] {
        let path = path
            .to_str()
            .ok_or("the pinned sysroot library path is not UTF-8")?;
        println!("cargo:rustc-link-search=native={path}");
        println!("cargo:rustc-link-arg-bin=rawr-import-driver=-Wl,-rpath,{path}");
    }
    Ok(())
}

/// Embed the host sources a generated harness compiles (every package's
/// library: its manifest, build script and `src`, less `#[cfg(test)]` module
/// files and binary sources) and their digest, which is also this Rawr's source
/// identity. Tests, benches, examples and binaries are never built for a
/// dependency, so editing them keeps the host.
fn embed_host() -> Result<(), Box<dyn Error>> {
    let repository = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").ok_or("no manifest dir")?)
        .join("..")
        .canonicalize()?;
    let output = PathBuf::from(env::var_os("OUT_DIR").ok_or("no OUT_DIR")?);
    let mut entries = Vec::new();
    for package in HOST_PACKAGES {
        let root = repository.join(package);
        let mut files = Vec::new();
        for name in ["build.rs", "build.template"] {
            let path = root.join(name);
            if path.is_file() {
                println!("cargo:rerun-if-changed={}", path.display());
                files.push(path);
            }
        }
        let source = root.join("src");
        println!("cargo:rerun-if-changed={}", source.display());
        collect(&source, &mut files)?;
        let manifest = root.join("Cargo.toml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        let (manifest_bytes, binaries) = library_manifest(&fs::read_to_string(&manifest)?);
        let mut excluded = vec![source.join("bin")];
        for binary in binaries {
            let binary = root.join(binary);
            excluded.push(binary.with_extension(""));
            excluded.push(binary);
        }
        for path in &files {
            if path.extension().is_some_and(|extension| extension == "rs") {
                test_modules(&source, path, &fs::read_to_string(path)?, &mut excluded);
            }
        }
        let manifest_copy = output.join(format!("{}.Cargo.toml", package.replace('/', "-")));
        if fs::read(&manifest_copy).ok().as_deref() != Some(manifest_bytes.as_slice()) {
            fs::write(&manifest_copy, &manifest_bytes)?;
        }
        entries.push((relative(&repository, &manifest)?, manifest_copy));
        for path in files {
            if !excluded.iter().any(|excluded| path.starts_with(excluded)) {
                entries.push((relative(&repository, &path)?, path));
            }
        }
    }
    entries.sort();
    let mut digest = Sha256::new();
    let mut table = String::from("pub(crate) static FILES: &[(&str, &[u8])] = &[\n");
    for (relative, path) in &entries {
        let bytes = fs::read(path)?;
        digest.update((relative.len() as u64).to_le_bytes());
        digest.update(relative.as_bytes());
        digest.update((bytes.len() as u64).to_le_bytes());
        digest.update(&bytes);
        writeln!(
            table,
            "    ({relative:?}, include_bytes!({:?})),",
            path.display().to_string()
        )?;
    }
    table.push_str("];\n");
    let digest: [u8; 32] = digest.finalize().into();
    fs::write(
        output.join("host.rs"),
        format!("pub const DIGEST: [u8; 32] = {digest:?};\n{table}"),
    )?;
    Ok(())
}

fn relative(repository: &Path, path: &Path) -> Result<String, Box<dyn Error>> {
    Ok(path
        .strip_prefix(repository)?
        .to_str()
        .ok_or("non-UTF-8 host path")?
        .to_owned())
}

/// The manifest without its `[[bin]]`, `[[bench]]`, `[[example]]` and
/// `[[test]]` targets, whose files the host leaves out, and the binaries'
/// declared paths.
fn library_manifest(text: &str) -> (Vec<u8>, Vec<String>) {
    let mut kept = String::with_capacity(text.len());
    let mut binaries = Vec::new();
    let mut target = None;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            target = ["[[bin]]", "[[bench]]", "[[example]]", "[[test]]"]
                .into_iter()
                .find(|header| trimmed == *header);
        }
        match target {
            None => kept.push_str(line),
            Some("[[bin]]") => {
                if let Some(path) = trimmed
                    .strip_prefix("path")
                    .and_then(|rest| rest.trim_start().strip_prefix('='))
                {
                    binaries.push(path.trim().trim_matches('"').to_owned());
                }
            }
            Some(_) => {}
        }
    }
    (kept.into_bytes(), binaries)
}

/// The file and directory of each `#[cfg(test)] mod name;` that `file`
/// declares (one with a `#[path]` attribute is kept).
fn test_modules(source: &Path, file: &Path, text: &str, excluded: &mut Vec<PathBuf>) {
    let directory = if file.ends_with("mod.rs")
        || file == source.join("lib.rs")
        || file == source.join("main.rs")
    {
        file.parent().unwrap_or(source).to_path_buf()
    } else {
        file.with_extension("")
    };
    let mut test = false;
    for line in text.lines().map(str::trim) {
        if line == "#[cfg(test)]" {
            test = true;
            continue;
        }
        if test && line.starts_with("#[") && !line.starts_with("#[path") {
            continue;
        }
        if test {
            let declaration = line
                .strip_prefix("pub(crate) ")
                .or_else(|| line.strip_prefix("pub(super) "))
                .or_else(|| line.strip_prefix("pub "))
                .unwrap_or(line);
            if let Some(name) = declaration
                .strip_prefix("mod ")
                .and_then(|name| name.strip_suffix(';'))
            {
                excluded.push(directory.join(format!("{name}.rs")));
                excluded.push(directory.join(name));
            }
        }
        test = false;
    }
}

fn collect(directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), Box<dyn Error>> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            collect(&path, files)?;
        } else if path.is_file() {
            files.push(path);
        }
    }
    Ok(())
}
