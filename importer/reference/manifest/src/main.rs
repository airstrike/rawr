#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::io::{self, Write as _};

use cargo_util_schemas::manifest;
use serde_json::json;

use crate::project::Project as _;

mod fixtures;
mod project;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args().skip(1);
    let mut selected = None;
    let mut list = false;
    match arguments.next().as_deref() {
        None => {}
        Some("--list") => list = true,
        Some("--case") => {
            selected = Some(arguments.next().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "--case requires a fixture id")
            })?)
        }
        Some(_) => {
            return Err(
                io::Error::new(io::ErrorKind::InvalidInput, "expected --list or --case ID").into(),
            )
        }
    }
    if arguments.next().is_some() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "unexpected argument").into());
    }
    let stdout = io::stdout();
    let mut output = io::BufWriter::new(stdout.lock());
    let mut count = 0;
    let mut mismatches = 0;
    for case in fixtures::cases() {
        if selected.as_deref().is_some_and(|id| id != case.id) {
            continue;
        }
        count += 1;
        let mut row = json!({"schema":1,"id":case.id,"purpose":case.purpose,
            "source":case.source,"expected_acceptance":case.accepted});
        if !list {
            let mut callbacks = Vec::new();
            let mut unused = BTreeSet::new();
            // This is the exact pinned Cargo entry sequence, not toml::from_str
            // or TomlManifest's Serialize round trip.
            let result = match toml::de::DeTable::parse(&case.source) {
                Ok(document) => {
                    let deserializer = toml::de::Deserializer::from(document.clone());
                    let result: Result<manifest::TomlManifest, toml::de::Error> =
                        serde_ignored::deserialize(deserializer, |path| {
                            let mut key = String::new();
                            stringify(&mut key, &path);
                            callbacks.push(key.clone());
                            unused.insert(key);
                        });
                    result.map_err(|error| ("typed", error))
                }
                Err(error) => Err(("document", error)),
            };
            let accepted = result.is_ok();
            if case.accepted.is_some_and(|expected| expected != accepted) {
                mismatches += 1;
            }
            row["accepted"] = json!(accepted);
            row["unused_callbacks"] = json!(callbacks);
            // On failure these are only observed callbacks, not a returned manifest.
            row["observed_unused"] = json!(unused);
            match result {
                Ok(mut manifest) => {
                    manifest._unused_keys = unused;
                    row["typed"] = manifest.project();
                    row["error"] = serde_json::Value::Null;
                }
                Err((phase, error)) => {
                    row["typed"] = serde_json::Value::Null;
                    row["error"] = json!({"phase":phase,"message":error.message(),
                        "display":error.to_string(),"span":error.span().map(|s| [s.start,s.end])});
                }
            }
        }
        serde_json::to_writer(&mut output, &row)?;
        output.write_all(b"\n")?;
    }
    output.flush()?;
    if count == 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "unknown fixture").into());
    }
    if mismatches != 0 {
        return Err(io::Error::other(format!("{mismatches} source-derived acceptance expectations differ; inspect emitted upstream evidence")).into());
    }
    Ok(())
}

// Copied from Cargo 59800466 src/cargo/util/toml/mod.rs:207–230.
// MIT OR Apache-2.0; retained licenses and member hash are in provenance.json.
fn stringify(dst: &mut String, path: &serde_ignored::Path<'_>) {
    use serde_ignored::Path;
    match *path {
        Path::Root => {}
        Path::Seq { parent, index } => {
            stringify(dst, parent);
            if !dst.is_empty() {
                dst.push('.');
            }
            dst.push_str(&index.to_string());
        }
        Path::Map { parent, ref key } => {
            stringify(dst, parent);
            if !dst.is_empty() {
                dst.push('.');
            }
            dst.push_str(key);
        }
        Path::Some { parent }
        | Path::NewtypeVariant { parent }
        | Path::NewtypeStruct { parent } => stringify(dst, parent),
    }
}
