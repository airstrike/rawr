//! What a compiler keeps of its resolutions from one compile to the next
//! (#1876).
//!
//! A unit's resolution is reused while the unit is as it was (its items,
//! its module, the item keys and block items it declares) and each read
//! it made of the crate's names ([`Tables`]) finds the same. Each one
//! reads too what every unit's does: the registry as the engine last
//! changed it, the program's package and entry, the native roots, and the
//! names of the traits two modules declare (`Every`). A reused
//! resolution does again what the resolution did besides its output: the
//! std traits it named by a path, the miss against native code it
//! recorded. What it gives is what resolving the unit again gives; a
//! reused item's shell is numbered as the program numbers its items now.
//!
//! A glob import's names are kept the same way, by its unit and prefix:
//! binding the globs reads again only those whose names could change,
//! a host module's children among them. So is each re-export a compile
//! found valid, by its address.
//!
//! [`Tables`]: super::tables::Tables

use std::collections::BTreeMap;

use super::tables::Reads;
use super::{
    Address, BlockItems, Context, Export, Failure, PatternOwners, ResolvedQualifiedCalls, Resolver,
};
use crate::package::source;
use crate::resolve::{item, std_trait, type_ref};
use crate::{program, ImmutableString, Span};

/// A compiler's kept resolutions.
pub(crate) struct Resolutions {
    /// What every kept resolution read besides the tables.
    every: Option<Every>,
    units: BTreeMap<source::Id, Unit>,
    /// The names of each glob import this compile read, by its unit, its
    /// prefix, and whether misses against native code were recorded.
    globs: BTreeMap<Glob, Memo<Option<Vec<ImmutableString>>>>,
    /// Those the compile before it read, which this one reuses.
    earlier: BTreeMap<Glob, Memo<Option<Vec<ImmutableString>>>>,
    /// Each re-export this compile found valid, by its address.
    exports: BTreeMap<Address, Memo<Export>>,
    /// Those the compile before it found valid.
    valid: BTreeMap<Address, Memo<Export>>,
}

/// A glob import of a unit, by its prefix, read where misses against
/// native code are recorded or not.
type Glob = (source::Id, Box<[ImmutableString]>, bool);

/// What every kept resolution reads besides the tables.
#[derive(PartialEq)]
struct Every {
    /// How many times the engine changed its registry.
    registry: u64,
    package: ImmutableString,
    entry: source::Id,
    hosted: bool,
    /// The crates a miss against native code is recorded for.
    roots: Option<Box<[ImmutableString]>>,
    trait_names: BTreeMap<Address, ImmutableString>,
}

/// One unit as it was resolved, and what its resolutions gave.
pub(super) struct Unit {
    context: Context,
    items: Box<[crate::item::Parsed]>,
    /// Each item's key, `None` for an item with no slot.
    keys: Box<[Option<Option<crate::item::Key>>]>,
    blocks: BlockItems,
    calls: Option<Memo<ResolvedQualifiedCalls>>,
    owners: Option<Memo<PatternOwners>>,
    resolved: Option<Memo<Resolved>>,
}

/// What a unit's own resolution gives (`Resolver::resolve_unit`).
#[derive(Clone)]
pub(super) struct Resolved {
    /// The names the blocks around the unit's derive bind.
    pub(super) entering: BTreeMap<ImmutableString, type_ref::Target>,
    /// Each derive of the unit's items, by its expansion's source.
    pub(super) sites: Vec<(source::Id, Span)>,
    pub(super) shells: Vec<item::Shell>,
    pub(super) aliases: BTreeMap<(program::source::Id, ImmutableString), type_ref::Alias>,
    /// The names the blocks around each derive of the unit's items bind,
    /// by its expansion's source.
    pub(super) around: Vec<(source::Id, BTreeMap<ImmutableString, type_ref::Target>)>,
    /// The names the crate root's items see, when the unit is the root.
    #[cfg_attr(
        not(all(feature = "rust-import", not(target_arch = "wasm32"))),
        allow(dead_code)
    )]
    pub(super) root: Option<BTreeMap<ImmutableString, type_ref::Target>>,
}

/// One resolution's output, and what it read and did.
pub(super) struct Memo<T> {
    trace: Trace,
    output: T,
}

/// What one resolution read and did besides its output.
struct Trace {
    reads: Reads,
    written: Vec<std_trait::Written>,
    #[cfg(feature = "rust-import")]
    miss: Option<crate::resolve::native::Miss>,
}

impl Resolutions {
    /// Nothing kept yet.
    pub(crate) const fn new() -> Self {
        Self {
            every: None,
            units: BTreeMap::new(),
            globs: BTreeMap::new(),
            earlier: BTreeMap::new(),
            exports: BTreeMap::new(),
            valid: BTreeMap::new(),
        }
    }

    /// Keep what `resolver` resolves: what every resolution reads, the
    /// registry as it was after `registry` changes and the native roots
    /// `roots`, and each unit. A unit that isn't as it was, and every unit
    /// when what every resolution reads isn't, is resolved again.
    pub(super) fn refresh(
        &mut self,
        resolver: &Resolver<'_>,
        registry: u64,
        roots: Option<Box<[ImmutableString]>>,
    ) {
        let every = Every {
            registry,
            package: resolver.package.clone(),
            entry: resolver.entry_source,
            hosted: resolver.hosted,
            roots,
            trait_names: resolver.trait_names.clone(),
        };
        self.earlier = std::mem::take(&mut self.globs);
        self.valid = std::mem::take(&mut self.exports);
        if self.every.as_ref() != Some(&every) {
            self.units.clear();
            self.earlier.clear();
            self.valid.clear();
            self.every = Some(every);
        }
        let mut units = std::mem::take(&mut self.units);
        for unit in &resolver.units {
            let source = unit.context.source;
            let keys = unit
                .slots
                .iter()
                .map(|slot| slot.as_ref().map(|slot| slot.key.clone()))
                .collect::<Box<[_]>>();
            let kept = units.remove(&source);
            if kept
                .as_ref()
                .is_some_and(|kept| kept.context != unit.context)
            {
                self.earlier.retain(|(at, ..), _| *at != source);
            }
            let kept = kept.filter(|kept| {
                kept.context == unit.context
                    && *kept.keys == *keys
                    && kept.blocks == unit.blocks
                    && *kept.items == *unit.parsed.items()
            });
            let kept = kept.unwrap_or_else(|| Unit {
                context: unit.context.clone(),
                items: unit.parsed.items().into(),
                keys,
                blocks: unit.blocks.clone(),
                calls: None,
                owners: None,
                resolved: None,
            });
            self.units.insert(source, kept);
        }
    }
}

impl Default for Resolutions {
    fn default() -> Self {
        Self::new()
    }
}

impl Unit {
    pub(super) fn calls(&mut self) -> &mut Option<Memo<ResolvedQualifiedCalls>> {
        &mut self.calls
    }

    pub(super) fn owners(&mut self) -> &mut Option<Memo<PatternOwners>> {
        &mut self.owners
    }

    pub(super) fn resolved(&mut self) -> &mut Option<Memo<Resolved>> {
        &mut self.resolved
    }
}

impl Resolver<'_> {
    /// What `resolve` gives for the unit `source`: what it gave when the
    /// unit's kept resolution in `memo` reads as it did and `fits` it, else
    /// what it gives now, which the unit keeps.
    pub(super) fn reused<T: Clone>(
        &self,
        source: source::Id,
        memo: fn(&mut Unit) -> &mut Option<Memo<T>>,
        fits: impl FnOnce(&T) -> bool,
        resolve: impl FnOnce() -> Result<T, Failure>,
    ) -> Result<T, Failure> {
        let Some(kept) = self.kept else {
            return resolve();
        };
        {
            let mut kept = kept.borrow_mut();
            if let Some(found) = kept
                .units
                .get_mut(&source)
                .and_then(|unit| memo(unit).as_ref())
                && fits(&found.output)
                && self.replayed(&found.trace)
            {
                return Ok(found.output.clone());
            }
        }
        let (resolved, trace) = self.traced(resolve);
        let output = resolved?;
        if let Some(unit) = kept.borrow_mut().units.get_mut(&source) {
            *memo(unit) = Some(Memo {
                trace,
                output: output.clone(),
            });
        }
        Ok(output)
    }

    /// Every name a glob import of `segments` reaches from `context`
    /// (`Resolver::names_of_glob`): what a kept glob's names were while
    /// what they read is as it was.
    pub(super) fn glob_names(
        &self,
        context: &Context,
        segments: &[ImmutableString],
    ) -> Option<Vec<ImmutableString>> {
        let Some(kept) = self.kept else {
            return self.names_of_glob(context, segments);
        };
        let key = (context.source, Box::from(segments), self.records_misses());
        {
            let mut kept = kept.borrow_mut();
            let kept = &mut *kept;
            if let Some(earlier) = kept.earlier.remove(&key) {
                kept.globs.insert(key.clone(), earlier);
            }
            if let Some(found) = kept.globs.get(&key)
                && self.replayed(&found.trace)
            {
                return found.output.clone();
            }
        }
        let (names, trace) = self.traced(|| self.names_of_glob(context, segments));
        kept.borrow_mut().globs.insert(
            key,
            Memo {
                trace,
                output: names.clone(),
            },
        );
        names
    }

    /// What `validate` gives for the re-export `export` at `address`:
    /// nothing, when the last compile found it valid and it reads as it did.
    pub(super) fn validated(
        &self,
        address: &Address,
        export: &Export,
        validate: impl FnOnce() -> Result<(), Failure>,
    ) -> Result<(), Failure> {
        let Some(kept) = self.kept else {
            return validate();
        };
        {
            let mut kept = kept.borrow_mut();
            let kept = &mut *kept;
            if let Some(valid) = kept.valid.remove(address)
                && valid.output == *export
                && self.replayed(&valid.trace)
            {
                kept.exports.insert(address.clone(), valid);
                return Ok(());
            }
        }
        let (validated, trace) = self.traced(validate);
        validated?;
        kept.borrow_mut().exports.insert(
            address.clone(),
            Memo {
                trace,
                output: export.clone(),
            },
        );
        Ok(())
    }

    /// What `resolve` gives, with what it read and did.
    fn traced<T>(&self, resolve: impl FnOnce() -> T) -> (T, Trace) {
        #[cfg(feature = "rust-import")]
        let recorded = self
            .native
            .as_deref()
            .map(super::super::native::Capture::recorded);
        let ((output, reads), written) = std_trait::writing(|| self.tables.recorded(resolve));
        let trace = Trace {
            reads,
            written,
            #[cfg(feature = "rust-import")]
            miss: self
                .native
                .as_deref()
                .zip(recorded)
                .and_then(|(capture, recorded)| capture.since(recorded)),
        };
        (output, trace)
    }

    /// Whether `trace` reads as it did; it does again what it did then.
    fn replayed(&self, trace: &Trace) -> bool {
        if !self.tables.hold(&trace.reads) {
            return false;
        }
        for (name, definition) in &trace.written {
            std_trait::written(name, definition);
        }
        #[cfg(feature = "rust-import")]
        if let Some((capture, miss)) = self.native.as_deref().zip(trace.miss.as_ref()) {
            capture.replay(miss);
        }
        true
    }

    /// Whether a lookup that misses native code records it.
    #[cfg(feature = "rust-import")]
    const fn records_misses(&self) -> bool {
        self.native.is_some()
    }

    #[cfg(not(feature = "rust-import"))]
    const fn records_misses(&self) -> bool {
        false
    }
}

#[cfg(all(test, feature = "rust-import"))]
mod tests {
    use super::Resolutions;
    use crate::resolve::{package, Program};
    use crate::Engine;

    const MAIN: &str = r#"// rawr
mod paint;
mod shapes;

use paint::*;
use shapes::{area, Named};

fn main() {
    let circle = shapes::circle(2.0);
    println!("{} {}", area(&circle), circle.name());
    let _ = shade(Shade::Light);
}
"#;

    const SHAPES: &str = r#"// rawr
pub struct Circle {
    pub radius: f64,
}

pub trait Named {
    fn name(&self) -> String;
}

impl Named for Circle {
    fn name(&self) -> String {
        "circle".to_string()
    }
}

pub fn circle(radius: f64) -> Circle {
    Circle { radius }
}

pub fn area(circle: &Circle) -> f64 {
    circle.radius * circle.radius
}
"#;

    const PAINT: &str = r#"// rawr
use super::shapes::Circle;
pub use super::shapes::circle;

pub enum Shade {
    Light,
    Dark,
}

pub fn shade(shade: Shade) -> Shade {
    shade
}

pub fn outline(circle: &Circle) -> f64 {
    circle.radius
}
"#;

    /// What `program` holds, written out.
    fn written(program: &Program) -> String {
        let scripts = program
            .scripts
            .iter()
            .map(|script| format!("{:?}|{:?}|{}", script.source, script.namespace, script.text))
            .collect::<Vec<_>>();
        format!(
            "{:?}\n{:?}\n{:?}\n{:?}\n{:?}\n{:?}\n{:?}\n{:?}\n{:?}\n{:?}\n{:?}\n{:?}\n{:?}\n\
             {:?}\n{:?}\n{:?}\n{:?}\n{:?}\n{:?}\n{:?}",
            program.package,
            program.items,
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
            program.mirrors,
            program.own,
            program.aliases,
            scripts,
            program.instances,
            program.lexical,
            program.macros,
        )
    }

    /// The program of the files `shapes` and `paint` beside `MAIN`'s
    /// text `main`, resolved with what `kept` holds, and resolved afresh.
    fn resolved(
        engine: &Engine,
        kept: &mut Resolutions,
        (main, shapes, paint): (&str, &str, &str),
    ) -> (String, String) {
        let manifest = "[package]\nname = \"shapes\"\nversion = \"0.1.0\"\nedition = \"2024\"\n";
        let files = vec![
            ("/shapes/Cargo.toml".into(), manifest.into()),
            ("/shapes/src/main.rs".into(), main.into()),
            ("/shapes/src/shapes.rs".into(), shapes.into()),
            ("/shapes/src/paint.rs".into(), paint.into()),
        ];
        let snapshot =
            crate::package::snapshot::Snapshot::new("/shapes/src/main.rs".into(), files).unwrap();
        let load = || crate::package::Graph::load_snapshot(engine, &snapshot).unwrap();
        let fresh = package::resolve(load(), &engine.host_registry)
            .unwrap_or_else(|failure| panic!("fresh resolution: {:?}", failure.error));
        let reused = package::resolve_kept(load(), &engine.host_registry, None, kept)
            .unwrap_or_else(|failure| panic!("kept resolution: {:?}", failure.error));
        (written(&reused), written(&fresh))
    }

    #[test]
    fn a_reused_resolution_is_the_fresh_one_after_each_edit() {
        let engine = Engine::new();
        let mut kept = Resolutions::new();
        // A field and a line above the item after it: the other units'
        // resolutions are reused.
        let field = SHAPES
            .replace(
                "    pub radius: f64,\n",
                "    pub radius: f64,\n    seen: bool,\n",
            )
            .replace("Circle { radius }", "Circle { radius, seen: false }");
        // A function the root's glob reaches: the root reads it.
        let added = PAINT.replace(
            "pub fn outline",
            "pub fn tint(shade: Shade) -> Shade {\n    shade\n}\n\npub fn outline",
        );
        // A renamed function, and its use in the root.
        let renamed = field.replace("pub fn area", "pub fn size");
        let main = MAIN
            .replace("{area, Named}", "{size, Named}")
            .replace("area(&circle)", "size(&circle)");
        let saves = [
            (MAIN, SHAPES, PAINT),
            (MAIN, SHAPES, PAINT),
            (MAIN, field.as_str(), PAINT),
            (MAIN, field.as_str(), added.as_str()),
            (main.as_str(), renamed.as_str(), added.as_str()),
            (MAIN, SHAPES, PAINT),
        ];
        for (save, files) in saves.into_iter().enumerate() {
            let (reused, fresh) = resolved(&engine, &mut kept, files);
            assert_eq!(reused, fresh, "save {save}");
        }
    }
}
