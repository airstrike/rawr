//! The lowerings a runner's compiler keeps from one save to the next
//! (#1646).
//!
//! A source's bodies are lowered again only when something their lowering
//! reads changed: the source's own text, or what the program holds of the
//! other sources. That is everything the resolved program holds but the
//! other sources' function and method bodies: their declarations with
//! their spans, the outputs reveal gave opaque types, the instance tables,
//! constants' bodies, `IndexMut` bodies and the trait objects the sources
//! name, and the program's item numbering. A program one of whose sources
//! reads outside it while it compiles (`include_str!`, `env!`) keeps none.

use super::entry::Lowered;
use super::models::Models;
use super::op::ExecutableOp;
use super::surface::ImportedUse;
use crate::program::source;
use crate::resolve::{item, Program};
use crate::{host, Engine, Position, Scope, Span};
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

/// The part of a source a kept lowering is.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum Part {
    /// Its functions' and methods' bodies.
    Bodies,
    /// The root's statements outside its items.
    Script,
}

/// The allocation one `retain_checked` charged.
pub(super) type Charge = (Option<usize>, Option<usize>);

/// What one part's lowering made, under the key it was made at.
pub(super) struct Kept {
    key: [u8; 32],
    pub(super) lowered: Box<[Lowered]>,
    pub(super) uses: Box<[ImportedUse]>,
    pub(super) dependencies: BTreeSet<host::facade::Dependency>,
    /// The allocations it charged, in order.
    pub(super) charges: Box<[Charge]>,
}

/// A compiler's kept lowerings (`Engine::resident_lowerings`).
pub(crate) struct Lowerings {
    kept: BTreeMap<(source::Id, Part), Kept>,
    /// How many sources this compiler's compiles lowered rather than
    /// reusing what an earlier compile lowered.
    lowered: usize,
}

impl Lowerings {
    /// No lowering kept yet.
    pub(crate) const fn new() -> Self {
        Self {
            kept: BTreeMap::new(),
            lowered: 0,
        }
    }

    /// How many sources this compiler's compiles lowered so far.
    pub(crate) fn lowered(&self) -> usize {
        self.lowered
    }
}

/// One compile's use of its compiler's kept lowerings. What it keeps goes
/// back to the compiler when it drops: on a failed compile, with what the
/// earlier compile kept of the sources it didn't reach.
pub(super) struct Session<'e> {
    engine: &'e Engine,
    /// What every source's lowering reads of the program; `None` when
    /// nothing is kept.
    interface: Option<[u8; 32]>,
    earlier: BTreeMap<(source::Id, Part), Kept>,
    kept: BTreeMap<(source::Id, Part), Kept>,
    lowered: BTreeSet<source::Id>,
}

impl<'e> Session<'e> {
    /// The session of a runner's compiler, which compiles the program
    /// again on each save; `None` for any other compiler.
    pub(super) fn open(
        engine: &'e Engine,
        program: &Program,
        models: &Models,
        scope: Option<&Scope>,
        optimization_level: crate::OptimizationLevel,
    ) -> Option<Self> {
        engine.imported_native.as_ref()?;
        let earlier = std::mem::take(&mut lock(engine).kept);
        let interface = scope
            .is_none_or(Scope::is_empty)
            .then(|| interface(program, models, optimization_level))
            .flatten();
        Some(Self {
            engine,
            interface,
            earlier,
            kept: BTreeMap::new(),
            lowered: BTreeSet::new(),
        })
    }

    /// The key of `part` of `source`, whose bodies' texts are `texts`:
    /// `None` when nothing is kept.
    pub(super) fn key(
        &self,
        part: Part,
        source: source::Id,
        program: &Program,
        texts: &[&str],
    ) -> Option<[u8; 32]> {
        let interface = self.interface?;
        let mut digest = Sha256::new();
        digest.update(b"rawr.lowering.source/v1");
        digest.update(interface);
        digest.update([u8::from(part == Part::Script)]);
        digest.update(source.index().to_le_bytes());
        framed(&mut digest, program.source.text_at(source));
        for text in texts {
            framed(&mut digest, text);
        }
        Some(digest.finalize().into())
    }

    /// What the earlier compile kept of `part` of `source` under `key`.
    pub(super) fn reuse(&mut self, part: Part, source: source::Id, key: [u8; 32]) -> Option<&Kept> {
        let kept = self
            .earlier
            .remove(&(source, part))
            .filter(|kept| kept.key == key)?;
        Some(self.kept.entry((source, part)).or_insert(kept))
    }

    /// Keep what `part` of `source` lowered to under `key`, when there is
    /// one.
    pub(super) fn keep(
        &mut self,
        part: Part,
        source: source::Id,
        key: Option<[u8; 32]>,
        lowered: &[Lowered],
        uses: &[ImportedUse],
        dependencies: &BTreeSet<host::facade::Dependency>,
        charges: Vec<Charge>,
    ) {
        self.lowered.insert(source);
        let Some(key) = key else {
            return;
        };
        self.kept.insert(
            (source, part),
            Kept {
                key,
                lowered: lowered.to_vec().into_boxed_slice(),
                uses: uses.to_vec().into_boxed_slice(),
                dependencies: dependencies.clone(),
                charges: charges.into_boxed_slice(),
            },
        );
    }

    /// The compile succeeded: what the earlier one kept and this one
    /// didn't reuse is gone.
    pub(super) fn succeeded(&mut self) {
        self.earlier.clear();
    }
}

impl Drop for Session<'_> {
    fn drop(&mut self) {
        let mut kept = std::mem::take(&mut self.earlier);
        kept.append(&mut self.kept);
        let mut lowerings = lock(self.engine);
        lowerings.kept = kept;
        lowerings.lowered += self.lowered.len();
    }
}

impl Lowered {
    /// This kept lowering for a new generation of the program: what an op
    /// holds of the generation it was made in made again. `None` when one
    /// can't be, and the body is lowered again.
    pub(super) fn renewed(&self, models: &Models) -> Option<Self> {
        let mut lowered = self.clone();
        lowered.ops = self
            .ops
            .iter()
            .map(|op| op.renewed(models))
            .collect::<Option<_>>()?;
        Some(lowered)
    }
}

/// An allocation that remembers what it charged, which a kept lowering
/// charges again when it is reused.
pub(super) struct Recording<'a> {
    pub(super) inner: &'a mut dyn host::ImportAllocation,
    pub(super) charges: Vec<Charge>,
}

impl host::ImportAllocation for Recording<'_> {
    fn retain_checked(
        &mut self,
        items: Option<usize>,
        bytes: Option<usize>,
    ) -> Result<(), rawr_import::surface::Error> {
        self.inner.retain_checked(items, bytes)?;
        self.charges.push((items, bytes));
        Ok(())
    }

    fn support_owner(&self) -> Option<crate::package::import::runner::account::support::Owner> {
        self.inner.support_owner()
    }
}

fn lock(engine: &Engine) -> std::sync::MutexGuard<'_, Lowerings> {
    engine
        .resident_lowerings
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// `text`, after its length, so two texts never run together.
fn framed(digest: &mut Sha256, text: &str) {
    digest.update(text.len().to_le_bytes());
    digest.update(text.as_bytes());
}

/// A digest written to as text, as `Debug` writes a value.
struct Written(Sha256);

impl std::fmt::Write for Written {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        self.0.update(text.as_bytes());
        Ok(())
    }
}

/// Whether a source may read outside the program while it compiles: a
/// file `include_str!` reads, a variable `env!` reads. Any text that names
/// either counts.
fn reads_outside(text: &str) -> bool {
    text.contains("include") || text.contains("env!")
}

/// What every source's lowering reads of the program, the other sources'
/// bodies aside; `None` when a source reads outside the program.
fn interface(
    program: &Program,
    models: &Models,
    optimization_level: crate::OptimizationLevel,
) -> Option<[u8; 32]> {
    if program.source.texts().any(reads_outside) {
        return None;
    }
    let mut written = Written(Sha256::new());
    written.0.update(b"rawr.lowering.interface/v1");
    // The compiler: its options and the facades its host registry holds.
    write!(
        written,
        "{optimization_level:?}|{}|{:?}|{:?}|{:?}|{}",
        Engine::registered_facades(),
        models.access,
        models.features,
        models.std_traits,
        models.test,
    )
    .ok()?;
    // Each source's text, its bodies blank: what a body's lowering reads
    // of another source is never its bodies, but their constants'.
    let mut bodies = BTreeMap::<source::Id, Vec<Span>>::new();
    for shell in program.items.iter() {
        if let item::Kind::Function(function) = &shell.kind
            && !function.body.constant
            && function.body.text.is_none()
        {
            bodies
                .entry(shell.source)
                .or_default()
                .push(function.body.span);
        }
    }
    for definition in program.methods.iter().chain(&program.builtin_methods) {
        if !definition.constant() && definition.body_text().is_none() {
            bodies
                .entry(definition.source())
                .or_default()
                .push(definition.body());
        }
    }
    for (index, text) in program.source.texts().enumerate() {
        let id = source::Id::from_index(index);
        framed(&mut written.0, program.source.identity_at(id));
        let spans = bodies.get(&id).map_or(&[][..], Vec::as_slice);
        framed(&mut written.0, &blank(text, spans));
    }
    // The program, its bodies' tokens aside: a body's spans and its text
    // when Rawr wrote it stay.
    for shell in program.items.iter() {
        match &shell.kind {
            item::Kind::Function(function) if !function.body.constant => {
                let mut function = function.clone();
                function.body.syntax = crate::item::BodySyntax::default();
                write!(
                    written,
                    "{:?}|{:?}|{:?}|{:?}|{:?}|{function:?}",
                    shell.id, shell.source, shell.key, shell.span, shell.facades,
                )
                .ok()?;
            }
            _ => write!(written, "{shell:?}").ok()?,
        }
    }
    write!(
        written,
        "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
        program.package,
        program.types,
        program.methods,
        program.builtin_methods,
        program.std_traits,
        program.qualified_functions,
        program.qualified_owners,
        program.qualified_pattern_owners,
        program.qualified_rust,
        program.qualified_traits,
        program.trait_names,
        program.entry_crate,
        program.crates,
        program.aliases,
    )
    .ok()?;
    #[cfg(feature = "rust-import")]
    write!(written, "{:?}", program.mirrors).ok()?;
    // Each source's statements outside its items, its items blank: by its
    // words, as a body's length changes how many blanks it is.
    for script in program.scripts.iter() {
        write!(written, "{:?}|{:?}|", script.source, script.namespace).ok()?;
        let words = script.text.split_whitespace().collect::<Vec<_>>();
        framed(&mut written.0, &words.join(" "));
    }
    write!(
        written,
        "{:?}|{:?}|{:?}|{:?}",
        program.instances, program.lexical, program.macros, models.index_mut,
    )
    .ok()?;
    Some(written.0.finalize().into())
}

/// `text` without each of `spans`, its line breaks kept: a body's length
/// changes nothing else.
fn blank(text: &str, spans: &[Span]) -> String {
    let mut output = String::with_capacity(text.len());
    let mut line = 1_u16;
    let mut column = 0_u16;
    for character in text.chars() {
        if character == '\n' {
            output.push(character);
            line = line.saturating_add(1);
            column = 0;
            continue;
        }
        column = column.saturating_add(1);
        let position = Position::new(line, column);
        let inside = spans
            .iter()
            .any(|span| position >= span.start() && position < span.end());
        if !inside {
            output.push(character);
        }
    }
    output
}

impl ExecutableOp {
    /// This op for a new generation of the program: a default value made
    /// again in its model, a witnessed op with no instance yet. `None` when
    /// the default can't be made.
    fn renewed(&self, models: &Models) -> Option<Self> {
        Some(match self {
            Self::Default { definition, .. } => Self::Default {
                value: models
                    .runtime
                    .materialize_default(
                        &crate::type_def::Identity::Script(definition.key().clone()),
                        definition.schema(),
                    )
                    .ok()?,
                definition: definition.clone(),
            },
            Self::Witnessed(op) => Self::Witnessed(Box::new(super::op::WitnessedOp::new(
                op.template.renewed(models)?,
                op.witnesses,
            ))),
            op => op.clone(),
        })
    }
}
