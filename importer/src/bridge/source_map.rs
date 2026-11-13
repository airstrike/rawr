//! Stable generated-span mapping to Rust declarations and Rawr use sites.

use std::collections::BTreeMap;

use serde::ser::SerializeSeq as _;
use serde::{Serialize, Serializer};

use crate::surface;

#[derive(Serialize)]
struct Record<'a> {
    identity: &'a [u8; 32],
    file: &'a str,
    generated: std::ops::Range<u32>,
    rust_source: &'a crate::api::model::Span,
    rust_declaration: &'a crate::api::model::Span,
    rawr: &'a surface::Origin,
}

/// Serialize source-map records without retaining a second copy of origins.
pub(crate) fn write<B, W>(
    request: &surface::Request<B>,
    spans: &[(
        surface::adapter::EntryIdentity,
        Box<str>,
        std::ops::Range<u32>,
    )],
    writer: W,
) -> Result<(), crate::bridge::request::Error>
where
    W: std::io::Write,
{
    let records = Borrowed { request, spans };
    serde_json::to_writer(writer, &records).map_err(crate::bridge::request::unwritable)
}

struct Borrowed<'a, B> {
    request: &'a surface::Request<B>,
    spans: &'a [(
        surface::adapter::EntryIdentity,
        Box<str>,
        std::ops::Range<u32>,
    )],
}

impl<B> Serialize for Borrowed<'_, B> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Each plan, and each identity's entries in their order, found once
        // rather than searched for at each span (#1832).
        let plans: BTreeMap<[u8; 32], &surface::adapter::Plan> = self
            .request
            .plans
            .iter()
            // The first of an identity, as a search finds it.
            .rev()
            .map(|plan| (*plan.identity().as_bytes(), plan))
            .collect();
        let mut entries = BTreeMap::<[u8; 32], Vec<&surface::Entry>>::new();
        for entry in &self.request.entries {
            entries
                .entry(*entry.identity().as_bytes())
                .or_default()
                .push(entry);
        }
        let entries_of = |identity: &[u8; 32]| entries.get(identity).map_or(&[][..], Vec::as_slice);
        let mut count = 0_usize;
        for plan in &self.request.plans {
            count = count.saturating_add(match plan.builder_steps() {
                Some(steps) => steps.len(),
                None => entries_of(plan.identity().as_bytes()).len(),
            });
        }
        let mut sequence = serializer.serialize_seq(Some(count))?;
        for (identity, file, generated) in self.spans {
            let plan = plans
                .get(identity.as_bytes())
                .ok_or_else(|| serde::ser::Error::custom("generated plan is absent"))?;
            if let Some(steps) = plan.builder_steps() {
                for step in steps {
                    sequence.serialize_element(&Record {
                        identity: identity.as_bytes(),
                        file,
                        generated: generated.clone(),
                        rust_source: step.plan().origin().source(),
                        rust_declaration: step.plan().origin().declaration(),
                        rawr: step.rawr(),
                    })?;
                }
                continue;
            }
            for entry in entries_of(identity.as_bytes()) {
                sequence.serialize_element(&Record {
                    identity: identity.as_bytes(),
                    file,
                    generated: generated.clone(),
                    rust_source: plan.origin().source(),
                    rust_declaration: plan.origin().declaration(),
                    rawr: entry.origin(),
                })?;
            }
        }
        sequence.end()
    }
}
