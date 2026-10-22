//! Complete API content separated from the current extraction derivation.

use serde::ser::{SerializeSeq as _, SerializeStruct as _};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::value::RawValue;

use crate::{Error, Result};

/// The exact aggregate metadata/unit/completion digests remain in the raw
/// Context and current resolution owner. This view retains their semantic
/// toolchain, selection, universe and exact final-lock authorities explicitly.
pub(crate) struct Context<'graph>(pub(crate) &'graph super::Context);

/// The attempt correlation and the artifact's physical bytes (`digest`,
/// `length`, `fresh`) are derivation evidence: a proc-macro dylib is not
/// bit-reproducible across build directories. The crate's `crate_hash`
/// (SVH) is derivation evidence too: it folds in every function body, so an
/// edit that leaves the extracted body unchanged would otherwise change the
/// API's semantics; what the API says about the crate is its extracted body,
/// and the crate is named by its `stable_crate_id`.
pub(crate) struct Invocations<'graph>(pub(crate) &'graph [super::Invocation]);

struct Invocation<'graph>(&'graph super::Invocation);

impl Serialize for Context<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let context = self.0;
        let mut value = serializer.serialize_struct("SemanticContext", 14)?;
        value.serialize_field("semantic_schema", &3_u32)?;
        value.serialize_field("graph_schema", &context.graph_schema)?;
        value.serialize_field("probe_schema", &context.probe_schema)?;
        value.serialize_field("canonicalizer", &context.canonicalizer)?;
        value.serialize_field("toolchain_channel", &context.toolchain_channel)?;
        value.serialize_field("compiler_commit", &context.compiler_commit)?;
        value.serialize_field("cargo_commit", &context.cargo_commit)?;
        value.serialize_field("host", &context.host)?;
        value.serialize_field("target", &context.target)?;
        value.serialize_field("profile", &context.profile)?;
        value.serialize_field("resolution_universe", &context.resolution_universe)?;
        value.serialize_field("final_lock", &context.final_lock)?;
        let unassociated = context.final_lock.is_none().then_some((
            &context.metadata,
            &context.build_unit,
            &context.resolution_proof,
        ));
        value.serialize_field("unassociated_derivation", &unassociated)?;
        value.serialize_field("selections", &context.selections)?;
        value.end()
    }
}

impl Serialize for Invocations<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut values = serializer.serialize_seq(Some(self.0.len()))?;
        for invocation in self.0 {
            values.serialize_element(&Invocation(invocation))?;
        }
        values.end()
    }
}

impl Serialize for Invocation<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let invocation = self.0;
        let mut value = serializer.serialize_struct("SemanticInvocation", 16)?;
        value.serialize_field("package_id", &invocation.package_id)?;
        value.serialize_field("root_aliases", &invocation.root_aliases)?;
        value.serialize_field("target", &invocation.target)?;
        value.serialize_field("target_kind", &invocation.target_kind)?;
        value.serialize_field("crate_types", &invocation.crate_types)?;
        value.serialize_field("crate_name", &invocation.crate_name)?;
        value.serialize_field("edition", &invocation.edition)?;
        value.serialize_field("compilation", &invocation.compilation)?;
        value.serialize_field("host", &invocation.host)?;
        value.serialize_field("selected_target", &invocation.selected_target)?;
        value.serialize_field("features", &invocation.features)?;
        value.serialize_field("artifact", &Artifact(&invocation.artifact))?;
        value.serialize_field("compiler_commit", &invocation.compiler_commit)?;
        value.serialize_field("unit_key", &invocation.unit_key)?;
        value.serialize_field("dependencies", &invocation.dependencies)?;
        value.serialize_field("sysroot", &invocation.sysroot)?;
        value.end()
    }
}

struct Artifact<'graph>(&'graph super::Artifact);

impl Serialize for Artifact<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut value = serializer.serialize_struct("SemanticArtifact", 2)?;
        value.serialize_field("profile", &self.0.profile)?;
        value.serialize_field("stable_crate_id", &self.0.stable_crate_id)?;
        value.end()
    }
}

/// The stored body stays borrowed while the current owner replaces its header.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Sections<'body> {
    schema: Option<u32>,
    #[serde(borrow)]
    context: &'body RawValue,
    #[serde(borrow)]
    invocations: &'body RawValue,
    #[serde(borrow)]
    definitions: &'body RawValue,
    #[serde(borrow)]
    rejected: Option<&'body RawValue>,
    #[serde(borrow)]
    exports: &'body RawValue,
    #[serde(borrow)]
    anonymous: &'body RawValue,
    #[serde(borrow)]
    implementations: &'body RawValue,
    #[serde(borrow)]
    scalar_predicate_proofs: &'body RawValue,
    #[serde(borrow)]
    support: Option<&'body RawValue>,
}

struct Current<'value> {
    sections: Sections<'value>,
    context: &'value super::Context,
    invocations: &'value [super::Invocation],
}

impl Serialize for Current<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let body = &self.sections;
        let fields = 7
            + usize::from(body.schema.is_some())
            + usize::from(body.rejected.is_some())
            + usize::from(body.support.is_some());
        let mut value = serializer.serialize_struct("RawCanonical", fields)?;
        if let Some(schema) = body.schema {
            value.serialize_field("schema", &schema)?;
        }
        value.serialize_field("context", self.context)?;
        value.serialize_field("invocations", self.invocations)?;
        value.serialize_field("definitions", body.definitions)?;
        if let Some(rejected) = body.rejected {
            value.serialize_field("rejected", rejected)?;
        }
        value.serialize_field("exports", body.exports)?;
        // Preserve both existing raw encoder orders exactly.
        if body.schema.is_some() {
            value.serialize_field("anonymous", body.anonymous)?;
        }
        value.serialize_field("implementations", body.implementations)?;
        value.serialize_field("scalar_predicate_proofs", body.scalar_predicate_proofs)?;
        if let Some(support) = body.support {
            value.serialize_field("support", support)?;
        }
        if body.schema.is_none() {
            value.serialize_field("anonymous", body.anonymous)?;
        }
        value.end()
    }
}

pub(super) fn current(
    bytes: &[u8],
    context: &super::Context,
    invocations: &[super::Invocation],
    limit: usize,
    allocation: &mut crate::extract::Allocation,
) -> Result<Vec<u8>> {
    let sections: Sections<'_> =
        serde_json::from_slice(bytes).map_err(|error| Error::Extraction {
            code: "semantic-api-sections",
            reason: error.to_string().into_boxed_str(),
        })?;
    require(sections.context.get().as_bytes(), &Context(context))?;
    require(
        sections.invocations.get().as_bytes(),
        &Invocations(invocations),
    )?;
    encode(
        &Current {
            sections,
            context,
            invocations,
        },
        limit,
        allocation,
    )
}

fn require(expected: &[u8], value: &impl Serialize) -> Result<()> {
    let mut exact = super::ExactCanonical {
        expected,
        offset: 0,
        matches: true,
    };
    serde_json::to_writer(&mut exact, value).map_err(|error| Error::Extraction {
        code: "semantic-api-association",
        reason: error.to_string().into_boxed_str(),
    })?;
    if !exact.matches || exact.offset != expected.len() {
        return Err(Error::Extraction {
            code: "semantic-api-association",
            reason: "current compiler header differs from stored API semantics".into(),
        });
    }
    Ok(())
}

/// Encode one semantic API document. `limit` is a physical bound: every
/// caller passes `node::MAX_CONTENT_BYTES` (directly or as
/// `Limits::node_canonical_bytes`), the node-store payload ceiling the encoded
/// document must fit, so exceeding it refuses. The accounting targets are
/// measured by `allocation.bytes`, which records a crossing and continues;
/// no caller may pass an accounting field here.
pub(crate) fn encode(
    value: &impl Serialize,
    limit: usize,
    allocation: &mut crate::extract::Allocation,
) -> Result<Vec<u8>> {
    let length = measure(value, limit, allocation)?;
    let mut output = Vec::new();
    output.try_reserve_exact(length).map_err(|source| {
        allocation.failure(Error::Allocation {
            operation: "semantic API encoding",
            source,
        })
    })?;
    append(value, length, &mut output)?;
    Ok(output)
}

/// Encode `value` in one pass under `encode`'s physical bound, without
/// recording it: the caller records the documents it composes from it.
pub(crate) fn encode_once(
    value: &impl Serialize,
    limit: usize,
    allocation: &mut crate::extract::Allocation,
) -> Result<Vec<u8>> {
    let mut limited = Limited {
        output: Vec::new(),
        limit,
        overflow: None,
        failed: None,
    };
    let result = serde_json::to_writer(&mut limited, value);
    if let Some(actual) = limited.overflow {
        return Err(allocation.failure(Error::Limit {
            resource: "semantic API bytes",
            limit,
            actual,
        }));
    }
    if let Some(source) = limited.failed {
        return Err(allocation.failure(Error::Allocation {
            operation: "semantic API encoding",
            source,
        }));
    }
    result.map_err(|error| Error::Extraction {
        code: "semantic-api-encoding",
        reason: error.to_string().into_boxed_str(),
    })?;
    Ok(limited.output)
}

/// Record one composed semantic document of `length` bytes as `measure` does.
pub(crate) fn admit(
    length: usize,
    limit: usize,
    allocation: &mut crate::extract::Allocation,
) -> Result<()> {
    if length > limit {
        return Err(allocation.failure(Error::Limit {
            resource: "semantic API bytes",
            limit,
            actual: length,
        }));
    }
    allocation.items(1)?;
    allocation.bytes(length)?;
    Ok(())
}

struct Limited {
    output: Vec<u8>,
    limit: usize,
    overflow: Option<usize>,
    failed: Option<std::collections::TryReserveError>,
}

impl std::io::Write for Limited {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let actual = self.output.len().saturating_add(buffer.len());
        if actual > self.limit {
            self.overflow = Some(actual);
            return Err(std::io::Error::other("semantic API byte limit exceeded"));
        }
        if let Err(error) = self.output.try_reserve(buffer.len()) {
            self.failed = Some(error);
            return Err(std::io::Error::other("semantic API allocation failed"));
        }
        self.output.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Measure one semantic API document under `encode`'s physical bound and
/// record it on the canonical account.
pub(crate) fn measure(
    value: &impl Serialize,
    limit: usize,
    allocation: &mut crate::extract::Allocation,
) -> Result<usize> {
    let mut measured = super::Measured {
        bytes: 0,
        limit,
        overflow: None,
    };
    let result = serde_json::to_writer(&mut measured, value);
    if let Some(actual) = measured.overflow {
        return Err(allocation.failure(Error::Limit {
            resource: "semantic API bytes",
            limit,
            actual,
        }));
    }
    result.map_err(|error| Error::Extraction {
        code: "semantic-api-encoding",
        reason: error.to_string().into_boxed_str(),
    })?;
    allocation.items(1)?;
    allocation.bytes(measured.bytes)?;
    Ok(measured.bytes)
}

/// Append one measured document to `output`, which already has room for it.
pub(crate) fn append(value: &impl Serialize, length: usize, output: &mut Vec<u8>) -> Result<()> {
    let start = output.len();
    serde_json::to_writer(&mut *output, value).map_err(|error| Error::Extraction {
        code: "semantic-api-encoding",
        reason: error.to_string().into_boxed_str(),
    })?;
    if output.len() - start != length {
        return Err(Error::Extraction {
            code: "semantic-api-size",
            reason: "semantic API encoding changed length".into(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn artifact_bytes_and_crate_hash_do_not_change_semantics_but_crate_identity_does() {
        let semantic = |digest: &str, crate_hash: &str, stable_crate_id: &str| {
            let invocation: super::super::Invocation = serde_json::from_value(serde_json::json!({
                "package_id": "p", "root_aliases": [], "target": "t", "target_kind": "proc-macro",
                "crate_types": ["proc-macro"], "crate_name": "m", "edition": "2021",
                "compilation": "c", "host": "h", "selected_target": "s", "features": [],
                "artifact": {"length": digest.len(), "digest": digest, "profile": "dev",
                    "fresh": digest == "a", "crate_hash": crate_hash, "stable_crate_id": stable_crate_id},
                "compiler_commit": "x", "unit_correlation": digest, "unit_key": "u",
                "dependencies": [], "sysroot": [],
            }))
            .expect("the invocation decodes");
            serde_json::to_vec(&super::Invocations(&[invocation])).expect("semantics encode")
        };
        assert_eq!(semantic("a", "h1", "i"), semantic("bb", "h1", "i"));
        // A body edit changes the SVH, not what the extracted API says.
        assert_eq!(semantic("a", "h1", "i"), semantic("a", "h2", "i"));
        assert_ne!(semantic("a", "h1", "i"), semantic("a", "h1", "j"));
    }
}
