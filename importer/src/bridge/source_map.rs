//! Stable generated-span mapping to Rust declarations and Rawr use sites.

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
    serde_json::to_writer(writer, &records).map_err(|_| crate::bridge::request::Error::Encoding)
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
        let mut count = 0_usize;
        for plan in &self.request.plans {
            count = count.saturating_add(match plan.builder_steps() {
                Some(steps) => steps.len(),
                None => self
                    .request
                    .entries
                    .iter()
                    .filter(|entry| entry.identity().as_bytes() == plan.identity().as_bytes())
                    .count(),
            });
        }
        let mut sequence = serializer.serialize_seq(Some(count))?;
        for (identity, file, generated) in self.spans {
            let plan = self
                .request
                .plans
                .iter()
                .find(|plan| plan.identity() == *identity)
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
            for entry in self
                .request
                .entries
                .iter()
                .filter(|entry| entry.identity().as_bytes() == identity.as_bytes())
            {
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
