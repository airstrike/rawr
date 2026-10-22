//! Direct pinned PackageIdSpec observations; never linked into the importer.

use std::collections::BTreeMap;
use std::io::{self, Write as _};

use cargo_util_schemas::core::{GitReference, PackageIdSpec, SourceKind};
use serde_json::{json, Value};

#[path = "spec/cases.rs"]
mod cases;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args().skip(1);
    if arguments.next().is_some() {
        return Err(io::Error::other("spec reference takes no arguments").into());
    }
    let stdout = io::stdout();
    let mut output = io::BufWriter::new(stdout.lock());
    let mut parsed = BTreeMap::new();
    let mut unexpected = false;
    for case in cases::CASES {
        let mut row = json!({
            "schema": 1, "row": "spec", "id": case.id, "input": case.input,
            "status": "error", "name": null, "version": null, "url": null,
            "kind": null, "rendered": null, "error": null, "error_debug": null,
        });
        // Catching the pinned panic is reference instrumentation only. Product
        // code uses an explicit source state at the second URL parse.
        match std::panic::catch_unwind(|| PackageIdSpec::parse(case.input)) {
            Ok(Ok(spec)) => {
                row["status"] = json!("ok");
                row["name"] = json!(spec.name());
                row["version"] = spec.partial_version().map_or(Value::Null, |version| {
                    json!({
                        "major": version.major, "minor": version.minor,
                        "patch": version.patch,
                        "pre": version.pre.as_ref().map(|value| value.as_str()),
                        "build": version.build.as_ref().map(|value| value.as_str()),
                    })
                });
                row["url"] = json!(spec.url().map(|url| url.as_str()));
                row["kind"] = spec.kind().map_or(Value::Null, kind);
                row["rendered"] = json!(spec.to_string());
                if parsed.insert(case.id, spec).is_some() {
                    return Err(io::Error::other("duplicate spec case id").into());
                }
                unexpected |= case.source;
            }
            Ok(Err(error)) => {
                row["error"] = json!(error.to_string());
                row["error_debug"] = json!(format!("{error:?}"));
                unexpected |= case.source;
            }
            Err(payload) => {
                let message = payload
                    .downcast_ref::<String>()
                    .map(String::as_str)
                    .or_else(|| payload.downcast_ref::<&str>().copied())
                    .unwrap_or("non-string panic payload");
                row["status"] = json!(if case.source { "source" } else { "panic" });
                row["error"] = json!(message);
                row["source_stage"] = json!(if case.source {
                    "url-reparse"
                } else {
                    "unexpected"
                });
                unexpected |= !case.source;
            }
        }
        serde_json::to_writer(&mut output, &row)?;
        output.write_all(b"\n")?;
    }

    let mut pairs = Vec::new();
    for &(left, right) in cases::PAIRS {
        let a = parsed
            .get(left)
            .ok_or_else(|| io::Error::other(format!("order input did not parse: {left}")))?;
        let b = parsed
            .get(right)
            .ok_or_else(|| io::Error::other(format!("order input did not parse: {right}")))?;
        let ordering = match a.cmp(b) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        };
        pairs.push(json!({"left": left, "right": right, "ordering": ordering, "equal": a == b}));
    }
    serde_json::to_writer(
        &mut output,
        &json!({"schema": 1, "row": "orders", "pairs": pairs}),
    )?;
    output.write_all(b"\n")?;
    output.flush()?;
    if unexpected {
        return Err(io::Error::other("unexpected reference panic or missing source-failure panic").into());
    }
    Ok(())
}

fn kind(value: &SourceKind) -> Value {
    let name = match value {
        SourceKind::Path => "path",
        SourceKind::Registry => "registry",
        SourceKind::SparseRegistry => "sparse",
        SourceKind::LocalRegistry => "local-registry",
        SourceKind::Directory => "directory",
        SourceKind::Git(reference) => {
            let (kind, value) = match reference {
                GitReference::Tag(value) => ("tag", value.as_str()),
                GitReference::Branch(value) => ("branch", value.as_str()),
                GitReference::Rev(value) => ("rev", value.as_str()),
                GitReference::DefaultBranch => ("default", ""),
            };
            return json!({"type": "git", "reference": {"kind": kind, "value": value}});
        }
    };
    json!({"type": name, "reference": null})
}
