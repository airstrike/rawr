use std::env;
use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest as _, Sha256};

#[path = "src/cargo/toolchain/host.rs"]
#[allow(dead_code)]
mod host;

#[path = "src/cargo/toolchain/window.rs"]
#[allow(dead_code)]
mod window;

/// The Rawr packages a generated harness builds against.
const HOST_PACKAGES: [&str; 10] = [
    ".",
    "canonical",
    "codegen",
    "directory",
    "importer",
    "location",
    "toml",
    "toml/edit",
    "watch",
    "watch/notify",
];

fn main() -> Result<(), Box<dyn Error>> {
    // Naming any path stops Cargo rerunning this script when it changes
    // itself, so it names itself too.
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/cargo/toolchain/host.rs");
    println!("cargo:rerun-if-changed=src/cargo/toolchain/window.rs");
    if env::var_os("CARGO_FEATURE_EMBEDDED_HOST").is_some() {
        embed_host()?;
    } else {
        hosted_digest()?;
    }
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_RUSTC_DRIVER");
    println!("cargo:rerun-if-env-changed=RUSTC");

    // The importer builds for the host of the toolchain that builds it.
    let rustc = env::var_os("RUSTC").ok_or("Cargo did not provide RUSTC")?;
    let version = Command::new(&rustc).arg("-vV").output()?;
    if !version.status.success() {
        return Err("the rustc version query failed".into());
    }
    let version = String::from_utf8(version.stdout)?;
    let host = host::parse(&version).ok_or("rustc -vV printed no host triple")?;
    // A file rather than `cargo:rustc-env`, because Cargo also sets such a
    // variable for test runs, where the importer reads the environment.
    let output = PathBuf::from(env::var_os("OUT_DIR").ok_or("no OUT_DIR")?);
    fs::write(output.join("toolchain-host"), host)?;
    compiler_cfgs(&version);
    if env::var_os("CARGO_FEATURE_RUSTC_DRIVER").is_none() {
        return Ok(());
    }
    // A driver built from rawr's own sources (`toolchain/driver.rs`) has no
    // `src/driver.rs` beside its manifest, and no sources to carry.
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").ok_or("no manifest dir")?);
    if manifest.join("src/driver.rs").is_file() {
        driver_sources(&manifest, &output)?;
    }

    let sysroot = Command::new(&rustc).args(["--print", "sysroot"]).output()?;
    if !sysroot.status.success() {
        return Err("the rustc sysroot query failed".into());
    }
    let sysroot = PathBuf::from(String::from_utf8(sysroot.stdout)?.trim());
    let library = sysroot.join("lib").canonicalize()?;
    let target_library = sysroot
        .join("lib/rustlib")
        .join(host)
        .join("lib")
        .canonicalize()?;
    for path in [&library, &target_library] {
        let path = path
            .to_str()
            .ok_or("the sysroot library path is not UTF-8")?;
        println!("cargo:rustc-link-search=native={path}");
        println!("cargo:rustc-link-arg-bin=rawr-import-driver=-Wl,-rpath,{path}");
    }
    Ok(())
}

/// The files the driver compiles, relative to the importer's directory, and
/// the root lockfile, which pins its dependencies.
const DRIVER_SOURCES: [&str; 9] = [
    "build.rs",
    "src/driver.rs",
    "src/driver",
    "src/record.rs",
    "src/extract/engine.rs",
    "src/extract/engine",
    "src/extract/protocol/attempt.rs",
    "src/cargo/toolchain/host.rs",
    "src/cargo/toolchain/window.rs",
];

/// The driver's sources, so `rawr-import` can build a driver for a nightly it
/// has none for (`toolchain/driver.rs`): their digest, which keys the built
/// drivers, and with `embedded-host` the files themselves, since an
/// installed `rawr-import` has no checkout. Without it, a build reads them
/// from this checkout.
fn driver_sources(importer: &Path, output: &Path) -> Result<(), Box<dyn Error>> {
    let mut files = Vec::new();
    for source in DRIVER_SOURCES {
        let path = importer.join(source);
        println!("cargo:rerun-if-changed={}", path.display());
        if path.is_dir() {
            collect(&path, &mut files)?;
        } else {
            files.push(path);
        }
    }
    let lock = importer.join("../Cargo.lock").canonicalize()?;
    println!("cargo:rerun-if-changed={}", lock.display());
    let mut entries = files
        .iter()
        .map(|path| {
            Ok((
                format!("importer/{}", relative(importer, path)?),
                path.clone(),
            ))
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    entries.push(("Cargo.lock".to_owned(), lock));
    entries.sort();
    let embed = env::var_os("CARGO_FEATURE_EMBEDDED_HOST").is_some();
    let mut digest = Sha256::new();
    let mut table = String::from("pub(crate) static FILES: &[(&str, &[u8])] = &[\n");
    let mut names = String::from("pub(crate) static NAMES: &[&str] = &[\n");
    for (relative, path) in &entries {
        let bytes = fs::read(path)?;
        digest.update((relative.len() as u64).to_le_bytes());
        digest.update(relative.as_bytes());
        digest.update((bytes.len() as u64).to_le_bytes());
        digest.update(&bytes);
        writeln!(names, "    {relative:?},")?;
        if embed {
            writeln!(
                table,
                "    ({relative:?}, include_bytes!({:?})),",
                path.display().to_string()
            )?;
        }
    }
    table.push_str("];\n");
    names.push_str("];\n");
    let digest: [u8; 32] = digest.finalize().into();
    let checkout = importer.join("..").canonicalize()?;
    fs::write(
        output.join("driver_source.rs"),
        format!(
            "pub(crate) const DIGEST: [u8; 32] = {digest:?};\n\
             pub(crate) const CHECKOUT: &str = {:?};\n{names}{table}",
            checkout.display().to_string()
        ),
    )?;
    Ok(())
}

/// The cfgs for the compiler APIs the building nightly has (`window.rs`),
/// and the commit the driver names in its version. A toolchain without a
/// commit date (a local build) gets the window's oldest APIs; one outside
/// the window gets `rawr_rustc_outside_window`, which the driver refuses.
fn compiler_cfgs(version: &str) {
    let value = |prefix: &str| {
        version
            .lines()
            .find_map(|line| line.strip_prefix(prefix))
            .map(str::trim)
            .unwrap_or_default()
    };
    let date = value("commit-date:");
    println!("cargo::rustc-check-cfg=cfg(rawr_rustc_outside_window)");
    for (cfg, _) in window::CHANGES {
        println!("cargo::rustc-check-cfg=cfg({cfg})");
    }
    // CI's nightly-window workflow sets RAWR_NIGHTLY_PROBE to learn whether
    // the window can move to a newer nightly: that nightly then builds with
    // every change's cfg instead of being refused.
    println!("cargo:rerun-if-env-changed=RAWR_NIGHTLY_PROBE");
    let probe = env::var_os("RAWR_NIGHTLY_PROBE").is_some() && date > window::NEWEST;
    if !date.is_empty() && !window::contains(date) && !probe {
        println!("cargo::rustc-cfg=rawr_rustc_outside_window");
    }
    println!("cargo::rustc-env=RAWR_RUSTC_WINDOW={}", window::RANGE);
    for (cfg, since) in window::CHANGES {
        if !date.is_empty() && date >= since {
            println!("cargo::rustc-cfg={cfg}");
        }
    }
    println!(
        "cargo::rustc-env=RAWR_RUSTC_COMMIT={}",
        value("commit-hash:")
    );
}

/// Embed the host sources a generated harness compiles (every package's
/// library: its manifest, build script, a `manifest.toml` its library
/// includes and `src`, less `#[cfg(test)]` module
/// files and binary sources, with the root's lockfile) and their digest, which is also this Rawr's source
/// identity. Tests, benches, examples and binaries are never built for a
/// dependency, so editing them keeps the host.
/// The digest of the sources this build carries when it builds a host
/// (`src/host.rs`), which the host's `.digest` stamp names: the digest of
/// the rawr that wrote it, so a runner built from the host knows the build
/// it came from. A development checkout has no stamp, and no digest.
fn hosted_digest() -> Result<(), Box<dyn Error>> {
    let stamp = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").ok_or("no manifest dir")?)
        .join("../.digest");
    println!("cargo:rerun-if-changed={}", stamp.display());
    let digest = fs::read_to_string(&stamp)
        .ok()
        .filter(|hex| hex.len() == 64)
        .and_then(|hex| {
            (0..32)
                .map(|index| u8::from_str_radix(hex.get(index * 2..index * 2 + 2)?, 16).ok())
                .collect::<Option<Vec<u8>>>()
        })
        .unwrap_or_else(|| vec![0; 32]);
    let output = PathBuf::from(env::var_os("OUT_DIR").ok_or("no OUT_DIR")?);
    fs::write(
        output.join("host.rs"),
        format!(
            "/// Identity of the Rawr sources this build carries.\npub const DIGEST: [u8; 32] = {digest:?};\n"
        ),
    )?;
    Ok(())
}

fn embed_host() -> Result<(), Box<dyn Error>> {
    let repository = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").ok_or("no manifest dir")?)
        .join("..")
        .canonicalize()?;
    let output = PathBuf::from(env::var_os("OUT_DIR").ok_or("no OUT_DIR")?);
    let mut entries = Vec::new();
    for package in HOST_PACKAGES {
        let root = repository.join(package);
        let mut files = Vec::new();
        // The root's lockfile is the one rawr's runtime resolves from, so a
        // release's runtime has the dependencies it was tested with (D620).
        let lock = if package == "." { "Cargo.lock" } else { "" };
        for name in ["build.rs", "build.template", "manifest.toml", lock] {
            if name.is_empty() {
                continue;
            }
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
        format!(
            "/// Identity of the Rawr sources this build carries.\npub const DIGEST: [u8; 32] = {digest:?};\n{table}"
        ),
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
