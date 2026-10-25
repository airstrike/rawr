//! The API content a capture's semantic inputs produced, remembered by those
//! inputs.
//!
//! The semantic API is a function of the compiler sidecars' bodies, their
//! handshakes' semantic fields, the semantic context, the request and the
//! extraction policy. An edit that leaves all of them unchanged (a function
//! body, a comment) would canonicalize the whole graph again only to reach the
//! same content; the memo names that content instead, and its graph is rebuilt
//! from it only if a consumer asks. A memo entry is the content digest; the
//! content is read from the store and checked against it, so a missing,
//! evicted or altered entry is a miss.

use std::path::PathBuf;

use sha2::{Digest as _, Sha256};

use super::cached::Product;
use crate::node::{self, Value as _};

/// The memo key for one capture, or `None` when it cannot be formed.
pub(crate) fn key(
    context: &super::Context,
    sidecars: &[(Vec<u8>, [u8; 32])],
    request: &[u8; 32],
    policy: &[u8; 32],
    store_context: &[u8; 32],
    limits: &crate::cargo::Limits,
) -> Option<[u8; 32]> {
    let semantic = serde_json::to_vec(&super::semantic::Context(context)).ok()?;
    let mut digest = Sha256::new();
    let mut field = |bytes: &[u8]| {
        digest.update((bytes.len() as u64).to_le_bytes());
        digest.update(bytes);
    };
    field(b"rawr.api.memo/v1");
    field(&super::CANONICALIZER_VERSION.to_le_bytes());
    field(&crate::extract::protocol::SCHEMA.to_le_bytes());
    field(&Product::SCHEMA.to_le_bytes());
    field(request);
    field(policy);
    field(store_context);
    field(format!("{limits:?}").as_bytes());
    field(&semantic);
    field(&(sidecars.len() as u64).to_le_bytes());
    for (projection, body) in sidecars {
        field(projection);
        field(body);
    }
    Some(digest.finalize().into())
}

fn path(context: &crate::context::Context, key: &[u8; 32]) -> PathBuf {
    context
        .rawr()
        .join("api-memo")
        .join(node::Digest::from_bytes(*key).directory())
}

/// The remembered content, read from the store and checked by its digest.
pub(crate) fn load(context: &crate::context::Context, key: &[u8; 32]) -> Option<Product> {
    let digest: [u8; 32] = std::fs::read(path(context, key)).ok()?.try_into().ok()?;
    let digest = node::Digest::from_bytes(digest);
    let store = context.open_store().ok()?;
    let payload = store
        .root()
        .join(crate::store::content_relative(
            Product::KIND,
            Product::SCHEMA,
            digest,
        ))
        .join("payload");
    let bytes = std::fs::read(payload).ok()?;
    if node::content_digest::<Product>(&bytes) != digest {
        return None;
    }
    Product::decode_owned(
        bytes.into_boxed_slice(),
        context.limits().node_canonical_bytes(),
    )
    .ok()
}

/// Remember the content these inputs produced; a failure only costs a miss.
pub(crate) fn remember(context: &crate::context::Context, key: &[u8; 32], content: node::Digest) {
    let path = path(context, key);
    let Some(directory) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(directory).is_err() {
        return;
    }
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    if std::fs::write(&temporary, content.as_bytes()).is_ok()
        && std::fs::rename(&temporary, &path).is_err()
    {
        let _ = std::fs::remove_file(&temporary);
    }
}
