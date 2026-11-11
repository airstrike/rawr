//! Rust's iterators. An adaptor chain (`v.iter().map(f).filter(p)`)
//! is a [`Pipeline`]: Rawr text for one loop per source that leaves each
//! item in a variable, with each stage's code in between. A consumer
//! (`sum`, `collect`, `find`, a `for` loop) runs the chain with its own code
//! at that point, so a chain is one pass with no array per stage, and stops
//! pulling where Rust's lazy iterator stops. An iterator bound to an
//! immutable local stays a chain: its sources and closures are evaluated
//! where it is bound, and each use runs it from there. An iterator passed,
//! returned or kept in a `let mut` is the array of its items at runtime.

use super::*;

/// An iterator chain not yet consumed.
#[derive(Clone)]
pub(super) struct Pipeline {
    /// Run once before the loops: closures, counts and other values, in
    /// the order Rust evaluates the chain.
    setup: origin::Fragment,
    /// The names `setup` binds.
    names: Vec<String>,
    /// The names its bindings so far bound (`bound_pipeline`), which its
    /// loops read: what a step function of it captures (`step_iterator`).
    outer: Vec<String>,
    /// Run before each run of the loops: the counters and flags the stages
    /// change, from their initial values.
    state: origin::Fragment,
    /// Each source in turn; `chain` appends one.
    segments: Vec<Segment>,
    /// The variable holding the current item.
    item: String,
    /// The flag a stage sets to end every loop, once a chain has inner or
    /// further loops; a lone loop ends with `break`.
    stop: Option<String>,
    /// The bare source while no stage follows it: what `zip` counts.
    source: Option<Source>,
    /// A one-loop source and the stages after it while each maps or drops
    /// items one by one (`map`, `filter`, `filter_map`, `inspect`): what
    /// `rev` runs over the source walked backwards, as Rust's
    /// `DoubleEndedIterator` for them does.
    reversible: Option<Reversible>,
    /// Whether it is Rust's `ExactSizeIterator`, which has `len()`.
    exact: bool,
}

#[derive(Clone)]
struct Reversible {
    source: Source,
    /// The variable holding the source's item.
    item: String,
    /// The stages' code, as a segment's `entry` and `exit`.
    entry: origin::Fragment,
    exit: String,
    /// Whether a stage drops items, so the source's length is not the
    /// chain's.
    dropping: bool,
}

#[derive(Clone)]
struct Segment {
    /// The flags that skip this loop: earlier sources ended the chain.
    guards: Vec<String>,
    /// `for x in v`, `loop`, `while i > 0`: text, and the values the loop
    /// evaluates where it starts.
    header: Vec<Part>,
    /// From the loop's `{` to the item: the stages' code.
    entry: origin::Fragment,
    /// From the item to the loop's `}`.
    exit: String,
}

#[derive(Clone)]
enum Part {
    Text(String),
    Value(origin::Fragment),
}

/// A loop header's code.
fn header(parts: &[Part]) -> origin::Fragment {
    let mut text = origin::Fragment::default();
    for part in parts {
        match part {
            Part::Text(part) => text.push_text(part),
            Part::Value(value) => text.push(value.clone()),
        }
    }
    text
}

#[derive(Clone)]
pub(super) enum Source {
    /// A value Rawr's `for` walks: an array (`array`), a string's chars or
    /// a range value.
    Values {
        values: origin::Fragment,
        array: bool,
    },
    /// `low..high`, `low..=high`, `low..`: of integers, or of chars
    /// (`char`), whose bounds are their code points and whose items the
    /// chain turns back into chars (`CHAR_OF_CODE`).
    Range {
        low: origin::Fragment,
        high: Option<origin::Fragment>,
        inclusive: bool,
        char: bool,
    },
}

/// The `Cast` code of a code point to its `char`, which a char range's
/// items are made by; one between the surrogates fails.
pub(super) const CHAR_OF_CODE: &str = "15";

/// std's iterator of a string's chars, which `str::chars` gives.
pub(super) const CHARS: &str = "core::str::iter::Chars";

impl Parser<'_> {
    /// The item of a range's source at `at`: its integer, or for a char
    /// range its char.
    fn range_item(&mut self, char: bool, at: &str) -> String {
        if char {
            let helper = self.push_op(super::Op::Prelude(super::PreludeOp::Cast));
            format!("{helper}({at}, {CHAR_OF_CODE})")
        } else {
            at.to_string()
        }
    }

    /// Whether `member` is an iterator method on a value of type `ty`:
    /// an iterator, or a range (`(0..=n).map(f)`), of a trait std declares
    /// it in that the value implements (`Iterator`, `DoubleEndedIterator`,
    /// `ExactSizeIterator`, `IntoIterator`).
    pub(super) fn iterator_receiver(&self, ty: &Ty, member: &str) -> bool {
        match self.inference.resolve(ty) {
            Ty::Iter(..) => true,
            ty @ Ty::InclusiveRange(_) => {
                let Some((definition, _)) = std_impl::std_definition(&ty) else {
                    return false;
                };
                std_method::iterator(member, |trait_| {
                    crate::std_table::impls(trait_, definition).next().is_some()
                })
                .is_some()
            }
            // A crate type's `impl Iterator`: the adaptors its own methods
            // do not name.
            ty @ Ty::Script(id, _) => {
                member != "next"
                    && self.models.method(id, member).is_none()
                    && self.type_implements(&ty, "Iterator")
                    && std_method::iterator(member, |trait_| {
                        self.type_implements(&ty, std_method::trait_name(trait_))
                    })
                    .is_some()
            }
            // An imported type the crate implements `Iterator` for, as
            // quick-xml's `Attributes` (D558 slice 2): the adaptors its own
            // methods do not name.
            ty @ Ty::Host { .. } => {
                member != "next"
                    && self.type_implements(&ty, "Iterator")
                    && std_method::iterator(member, |trait_| {
                        self.type_implements(&ty, std_method::trait_name(trait_))
                    })
                    .is_some()
                    && ty.host_alias().is_none_or(|definition| {
                        self.models
                            .host_items(&format!("{}::{member}", definition.path()))
                            .is_none()
                    })
            }
            _ => false,
        }
    }

    /// A chain over a crate iterator: its `next()` until `None`. `id` is
    /// the script type's, or `None` for an imported type the crate
    /// implements `Iterator` for, whose `next` is a native call (D558
    /// slice 2).
    fn user_pipeline(
        &mut self,
        value: &ParsedExpr,
        id: Option<type_def::Id>,
    ) -> Result<(Pipeline, Ty), Error> {
        let ty = self.inference.resolve(&value.ty);
        // The `next` of a crate type several paths name, as itertools'
        // `Product`, which no alias has yet: the miss the importer answers
        // by bringing it.
        #[cfg(feature = "rust-import")]
        if id.is_none()
            && let Some(error) = self.ambiguous_imported_member(&ty, "next", value.span, true)
        {
            return Err(error);
        }
        let iterator = self.temporary("iterator");
        self.bindings.insert(
            iterator.as_str().into(),
            Binding {
                ty: ty.clone(),
                mutable: true,
                available: true,
                declaration: value.span,
                kind: BindingKind::Local,
            },
        );
        let receiver = ParsedExpr {
            prelude: origin::Fragment::default(),
            source: origin::Fragment::plain(iterator.clone()),
            ty: ty.clone(),
            span: value.span,
            lvalue: Some(Lvalue::Binding {
                binding: iterator.as_str().into(),
                ty: ty.clone(),
            }),
            outcome: crate::host::usage::Outcome::Conservative,
            evaluation: crate::host::usage::Evaluation::Required,
        };
        let called = match id {
            Some(id) => {
                let name = self.models.definition(id).name().into();
                self.call_owner_method(
                    receiver,
                    id,
                    name,
                    &"next".into(),
                    value.span,
                    Vec::new(),
                    value.span.end(),
                )
            }
            None => {
                self.pending_arguments = Some((Vec::new(), value.span.end()));
                self.parse_host_method(receiver, "next".into(), value.span)
                    .map(Ok)
            }
        };
        self.bindings.remove(iterator.as_str());
        let Ok(called) = called? else {
            return Err(self.unsupported("an `Iterator` impl without `next`"));
        };
        let Ty::Option(item) = self.inference.resolve(&called.ty) else {
            return Err(self.unsupported("an `Iterator` whose `next` is not an `Option`"));
        };
        let (item_name, option) = (self.temporary("item"), self.temporary("option"));
        // The iterator as made, and its copy each run advances.
        let initial = self.temporary("initial");
        let mut setup = origin::Fragment::plain(format!("let {initial} = "));
        setup.push(value.rendered_source());
        setup.push_text("; ");
        let mut entry = origin::Fragment::plain(format!("let {option} = "));
        entry.push(called.rendered_source());
        entry.push_text("; if !");
        let test = self.prelude_op(PreludeOp::IsSome, [option.as_str().into()]);
        entry.push(test);
        entry.push_text(format!(" {{ break; }} let {item_name} = "));
        let unwrapped = self.prelude_op(PreludeOp::Unwrap, [option.as_str().into()]);
        entry.push(unwrapped);
        entry.push_text("; ");
        let pipeline = Pipeline {
            setup,
            names: vec![initial.clone()],
            outer: Vec::new(),
            state: origin::Fragment::plain(format!("let {iterator} = {initial}; ")),
            segments: vec![Segment {
                guards: Vec::new(),
                header: vec![Part::Text("loop".into())],
                entry,
                exit: String::new(),
            }],
            item: item_name,
            stop: None,
            source: None,
            reversible: None,
            exact: false,
        };
        Ok((pipeline, *item))
    }

    /// A program's iterator where a crate takes an iterator, as itertools'
    /// `rows.cartesian_product(columns)` takes both (`future::Items`): its
    /// chain as a step function that runs the chain and suspends at each
    /// item (`eval::machine`), so native code pulls one item on each
    /// `next()`, and the chain's closures run as it pulls. The chain's
    /// sources and closures are evaluated here, where Rust evaluates them;
    /// its counters start with each run, as a clone of an iterator that
    /// hasn't started starts again. `false`, and `value` unchanged, when it
    /// is no iterator of the program's: a crate's own iterator crosses as
    /// itself.
    #[cfg(feature = "rust-import")]
    pub(super) fn step_iterator(&mut self, value: &mut ParsedExpr) -> Result<bool, Error> {
        if matches!(self.inference.resolve(&value.ty), Ty::Host { .. }) || self.callable.is_none() {
            return Ok(false);
        }
        let Some((pipeline, item)) = self.pipeline_of(value) else {
            return Ok(false);
        };
        let (code, pipeline, _) = self.bound_pipeline(pipeline);
        let Some(callable) = &self.callable else {
            return Ok(false);
        };
        self.closure_count += 1;
        let key = crate::item::Key::closure(callable, self.closure_count);
        let sink = origin::Fragment::plain(format!(
            "{}({}); ",
            crate::eval::machine::AWAIT,
            pipeline.item
        ));
        let mut body = origin::Fragment::plain("{ ");
        body.push(Self::render(&pipeline, &sink));
        body.push_text("}");
        body.cover(value.span);
        let mut captured = BTreeSet::new();
        let mut parameters = pipeline
            .outer
            .iter()
            .filter(|name| captured.insert(name.as_str()))
            .map(|name| ImmutableString::from(name.as_str()))
            .collect::<Vec<_>>();
        let captures = parameters
            .iter()
            .map(|name| origin::Fragment::plain(name.as_str()))
            .collect();
        parameters.push(self.temporary("resume").into());
        self.lifted.push(closure::Lifted {
            key: key.clone(),
            value: None,
            parameters,
            captures,
            body,
            span: value.span,
            header: value.span,
            held: false,
        });
        let mut prelude = std::mem::take(&mut value.prelude);
        prelude.push(code);
        let mut source = origin::Fragment::plain("()");
        source.cover(value.span);
        let mut step = ParsedExpr {
            prelude,
            source,
            ty: Ty::FnItem {
                key: Box::new(key),
                inputs: Box::new([Ty::Unit]),
                output: Box::new(Ty::Unit),
            },
            span: value.span,
            lvalue: None,
            outcome: crate::host::usage::Outcome::Conservative,
            evaluation: crate::host::usage::Evaluation::Required,
        };
        self.host_callable(&mut step)?;
        let callable = std::mem::replace(&mut step.ty, Ty::iter(item));
        self.steps
            .insert(step.source.as_str().to_string(), callable);
        *value = step;
        Ok(true)
    }

    /// `v.iter()`, `v.into_iter()`: a chain over an array's items.
    pub(super) fn array_iterator(
        &mut self,
        values: &ParsedExpr,
        item: Ty,
        end: Position,
    ) -> ParsedExpr {
        let source = Source::Values {
            values: values.rendered_source(),
            array: true,
        };
        let pipeline = self.source_pipeline(source);
        self.iterator_expr(pipeline, item, span(values.span.start(), end))
    }

    /// `o.iter()` and `o.into_iter()` of an `Option` or `Result`, std's
    /// `IntoIterator` for them: a chain over the one value `held` gives
    /// where `present` holds, else over none. `opened` binds the receiver
    /// and stays open for the block this closes.
    pub(super) fn held_iterator(
        &mut self,
        mut opened: origin::Fragment,
        [present, held]: [origin::Fragment; 2],
        item: Ty,
        expression: Span,
    ) -> ParsedExpr {
        opened.push_text("if ");
        opened.push(present);
        opened.push_text(" { [");
        opened.push(held);
        opened.push_text("] } else { [] } }");
        let array = Ty::Array {
            element: Box::new(item.clone()),
            kind: sequence::Kind::Vec,
        };
        let values = self.operator_expr(opened.surrounded("(", ")"), array, expression);
        self.array_iterator(&values, item, expression.end())
    }

    /// The item `member` of an `Option<T>` or `Result<T, E>` `receiver`
    /// iterates: `&T` of `iter`, and of `into_iter` through a reference
    /// (`&mut T` through `&mut`), else `T`.
    pub(super) fn held_item(&self, receiver: &Ty, member: &str, element: Ty) -> Ty {
        match (member, self.inference.resolve(receiver)) {
            ("into_iter", Ty::Reference { mutable, .. }) => Ty::Reference {
                mutable,
                target: Box::new(element),
            },
            ("into_iter", _) => element,
            _ => Ty::Reference {
                mutable: false,
                target: Box::new(element),
            },
        }
    }

    /// `s.chars()`: a chain over a string's chars.
    pub(super) fn chars_iterator(&mut self, value: &ParsedExpr, end: Position) -> ParsedExpr {
        let source = Source::Values {
            values: value.rendered_source(),
            array: false,
        };
        let pipeline = self.source_pipeline(source);
        let mut expr = self.iterator_expr(pipeline, Ty::Char, span(value.span.start(), end));
        expr.ty = Ty::Iter(Box::new(Ty::Char), Some(CHARS));
        expr
    }

    /// `low..high`, `low..` and, kept as Rawr's range value, `low..=high`.
    pub(super) fn range_iterator(
        &mut self,
        low: &ParsedExpr,
        high: Option<&ParsedExpr>,
        item: Ty,
        inclusive: bool,
    ) -> ParsedExpr {
        // A char range walks its bounds' code points.
        let char = item == Ty::Char;
        let bound = |this: &mut Self, value: &ParsedExpr| {
            if char {
                this.prelude_op(
                    super::PreludeOp::Cast,
                    [value.rendered_source(), "7".into()],
                )
            } else {
                value.rendered_source()
            }
        };
        let source = Source::Range {
            low: bound(self, low),
            high: high.map(|high| bound(self, high)),
            inclusive,
            char,
        };
        let pipeline = self.source_pipeline(source);
        let end = high.map_or(low.span.end(), |high| high.span.end());
        // An integer range is Rawr's range value of its kind, but for an `i128`
        // or a `u128`, whose bounds may be wider than its `i64`s.
        let kind = match self.inference.resolve(&item) {
            Ty::Int(kind) if !kind.wide() => Some(kind),
            _ => None,
        };
        let mut expr = self.iterator_expr(pipeline.clone(), item, span(low.span.start(), end));
        if inclusive && let Some(kind) = kind {
            // `a..=b` stays the range value hosts take; its chain is kept.
            let mut value = low.rendered_source();
            value.push_text(" ..= ");
            value.push(
                high.expect("an inclusive range has an end")
                    .rendered_source(),
            );
            expr.source = value;
            expr.ty = Ty::InclusiveRange(kind);
            expr = expr.retain_origin();
            self.pipelines
                .insert(expr.source.as_str().to_string(), pipeline);
        }
        expr
    }

    /// Keep a parenthesized iterator's chain: `(0..n).map(f)`.
    pub(super) fn keep_pipeline(&mut self, before: &str, after: &ParsedExpr) {
        if let Some(pipeline) = self.pipelines.get(before).cloned() {
            self.pipelines
                .insert(after.source.as_str().to_string(), pipeline);
        }
    }

    pub(super) fn source_pipeline(&mut self, source: Source) -> Pipeline {
        let item = self.temporary("item");
        let mut pipeline = Pipeline {
            setup: origin::Fragment::default(),
            names: Vec::new(),
            outer: Vec::new(),
            state: origin::Fragment::default(),
            segments: Vec::new(),
            item: item.clone(),
            stop: None,
            source: None,
            reversible: None,
            exact: false,
        };
        let (header, entry) = match &source {
            Source::Values { values, .. } => (
                vec![
                    Part::Text(format!("for {item} in (")),
                    Part::Value(values.clone()),
                    Part::Text(")".into()),
                ],
                origin::Fragment::default(),
            ),
            Source::Range {
                low,
                high: Some(high),
                inclusive,
                char,
            } => {
                let code = if *char {
                    self.temporary("code")
                } else {
                    item.clone()
                };
                let entry = if *char {
                    origin::Fragment::plain(format!(
                        "let {item} = {}; ",
                        self.range_item(true, &code)
                    ))
                } else {
                    origin::Fragment::default()
                };
                (
                    vec![
                        Part::Text(format!("for {code} in (")),
                        Part::Value(low.clone()),
                        Part::Text(if *inclusive { ")..=(" } else { ")..(" }.into()),
                        Part::Value(high.clone()),
                        Part::Text(")".into()),
                    ],
                    entry,
                )
            }
            Source::Range {
                low,
                high: None,
                char,
                ..
            } => {
                let start = self.bind(&mut pipeline, "start", low.clone());
                let next = self.bind_state(&mut pipeline, "next", start.into());
                (
                    vec![Part::Text("loop".into())],
                    origin::Fragment::plain(format!(
                        "let {item} = {}; {next} += 1; ",
                        self.range_item(*char, &next)
                    )),
                )
            }
        };
        pipeline.segments.push(Segment {
            guards: Vec::new(),
            header,
            entry,
            exit: String::new(),
        });
        if !matches!(source, Source::Range { high: None, .. }) {
            pipeline.reversible = Some(Reversible {
                source: source.clone(),
                item,
                entry: origin::Fragment::default(),
                exit: String::new(),
                dropping: false,
            });
        }
        pipeline.exact = matches!(
            source,
            Source::Values { array: true, .. } | Source::Range { high: Some(_), .. }
        );
        pipeline.source = Some(source);
        pipeline
    }

    /// The iterator expression of `pipeline`: the array of its items, its
    /// chain kept for the consumer that fuses it.
    pub(super) fn iterator_expr(
        &mut self,
        pipeline: Pipeline,
        item: Ty,
        expression: Span,
    ) -> ParsedExpr {
        let source = self.materialized(&pipeline);
        let expr = self.operator_expr(source, Ty::iter(item), expression);
        self.pipelines
            .insert(expr.source.as_str().to_string(), pipeline);
        expr
    }

    /// A chain over a map's or set's entries: each pair, or (`project`)
    /// its key (`0`) or value (`1`).
    pub(super) fn keyed_iterator(
        &mut self,
        values: &ParsedExpr,
        item: Ty,
        project: Option<usize>,
        end: Position,
    ) -> ParsedExpr {
        let mut pipeline = self.source_pipeline(Source::Values {
            values: values.rendered_source(),
            array: true,
        });
        if let Some(index) = project {
            let next = self.temporary("item");
            let open = format!("let {next} = {}[{index}]; ", pipeline.item);
            pipeline.item = next;
            Self::stage(&mut pipeline, &open.into(), "");
        }
        self.iterator_expr(pipeline, item, span(values.span.start(), end))
    }

    /// `value` as Rust's `IntoIterator` turns it into an iterator, when it
    /// is iterable.
    pub(super) fn iterator_of(&mut self, value: &ParsedExpr) -> Option<ParsedExpr> {
        let (pipeline, item) = self.pipeline_of(value)?;
        Some(self.iterator_expr(pipeline, item, value.span))
    }

    /// The array of what `value` yields, and its item type: an array as it
    /// is, any other iterable's chain collected.
    pub(super) fn collected_items(&mut self, value: &ParsedExpr) -> Option<(origin::Fragment, Ty)> {
        if let Ty::Array { element, .. } = self.inference.resolve(&value.ty) {
            return Some((value.rendered_source(), *element));
        }
        let (pipeline, item) = self.pipeline_of(value)?;
        Some((self.materialized(&pipeline), item))
    }

    /// Why `value` has no chain: for a crate type that implements
    /// `Iterator`, the miss of its `next`, which the importer answers by
    /// bringing the member as it brings any other (#709); else `fallback`.
    pub(super) fn pipeline_error(&mut self, value: &ParsedExpr, fallback: Error) -> Error {
        let ty = self.inference.resolve(&value.ty);
        if matches!(ty, Ty::Host { .. })
            && self.type_implements(&ty, "Iterator")
            && let Err(error) = self.user_pipeline(value, None)
        {
            return error;
        }
        fallback
    }

    /// The chain of `value` and its item type, when it is an iterator or
    /// something Rust's `IntoIterator` walks (an array, `&Vec`, a range).
    fn pipeline_of(&mut self, value: &ParsedExpr) -> Option<(Pipeline, Ty)> {
        let ty = self.inference.resolve(&value.ty);
        if let Ty::Script(id, _) = ty
            && self.type_implements(&ty, "Iterator")
        {
            return self.user_pipeline(value, Some(id)).ok();
        }
        if matches!(ty, Ty::Host { .. }) && self.type_implements(&ty, "Iterator") {
            return self.user_pipeline(value, None).ok();
        }
        let item = match &ty {
            Ty::Iter(item, _) => (**item).clone(),
            Ty::InclusiveRange(kind) => Ty::Int(*kind),
            Ty::Array { element, .. } => (**element).clone(),
            // A map's or set's sorted entries.
            Ty::Keyed(kind, arguments) => keyed::keyed_item(*kind, arguments, None),
            Ty::Reference { mutable, target } => match &**target {
                Ty::Array { element, .. } => Ty::Reference {
                    mutable: *mutable,
                    target: element.clone(),
                },
                Ty::Keyed(kind, arguments) => keyed::keyed_item(*kind, arguments, Some(*mutable)),
                _ => return None,
            },
            _ => return None,
        };
        if value.prelude.is_empty()
            && let Some(pipeline) = self.pipelines.get(value.source.as_str())
        {
            return Some((pipeline.clone(), item));
        }
        let source = Source::Values {
            values: value.rendered_source(),
            array: !matches!(ty, Ty::InclusiveRange(_)),
        };
        Some((self.source_pipeline(source), item))
    }

    /// What ends every loop of the chain.
    fn stop_text(pipeline: &Pipeline) -> String {
        match &pipeline.stop {
            Some(flag) => format!("{{ {flag} = true; break; }}"),
            None => "{ break; }".into(),
        }
    }

    /// Give the chain a stop flag, before a stage that adds loops.
    fn nest(&mut self, pipeline: &mut Pipeline) -> String {
        if let Some(flag) = &pipeline.stop {
            return flag.clone();
        }
        let flag = self.bind_state(pipeline, "stop", "false".into());
        pipeline.stop = Some(flag.clone());
        flag
    }

    /// Add a stage: `open` runs on each item before the rest of the chain,
    /// `close` after it.
    fn stage(pipeline: &mut Pipeline, open: &origin::Fragment, close: &str) {
        for segment in &mut pipeline.segments {
            segment.entry.push(open.clone());
            segment.exit.insert_str(0, close);
        }
        pipeline.source = None;
        pipeline.reversible = None;
    }

    /// A stage that maps or drops (`dropping`) each item by itself, which
    /// `rev` keeps.
    fn item_stage(pipeline: &mut Pipeline, open: &origin::Fragment, dropping: bool) {
        let reversible = pipeline.reversible.take().map(|mut reversible| {
            reversible.entry.push(open.clone());
            reversible.dropping |= dropping;
            reversible
        });
        Self::stage(pipeline, open, "");
        pipeline.reversible = reversible;
    }

    /// Bind `value` once in the chain's setup; its name.
    fn bind(&mut self, pipeline: &mut Pipeline, role: &str, value: origin::Fragment) -> String {
        let name = self.temporary(role);
        pipeline.setup.push_text(format!("let {name} = "));
        pipeline.setup.push(value);
        pipeline.setup.push_text("; ");
        pipeline.names.push(name.clone());
        name
    }

    /// Bind `value` before each run of the chain: a counter or flag a
    /// stage changes; its name.
    fn bind_state(
        &mut self,
        pipeline: &mut Pipeline,
        role: &str,
        value: origin::Fragment,
    ) -> String {
        let name = self.temporary(role);
        pipeline.state.push_text(format!("let {name} = "));
        pipeline.state.push(value);
        pipeline.state.push_text("; ");
        name
    }

    /// An immutable local's chain: the code that evaluates its
    /// setup and the values its loops start from, where it is bound; the
    /// chain each use runs from there; and the names that code binds.
    pub(super) fn bound_pipeline(
        &mut self,
        mut pipeline: Pipeline,
    ) -> (origin::Fragment, Pipeline, Vec<String>) {
        let mut code = std::mem::take(&mut pipeline.setup);
        let mut names = std::mem::take(&mut pipeline.names);
        for segment in &mut pipeline.segments {
            for part in &mut segment.header {
                let Part::Value(value) = part else {
                    continue;
                };
                let name = self.temporary("source");
                code.push_text(format!("let {name} = "));
                code.push(value.clone());
                code.push_text("; ");
                *part = Part::Value(origin::Fragment::plain(name.clone()));
                names.push(name);
            }
        }
        // Its walks start from the bound values too.
        let rebind = |source: &mut Source, segment: &Segment| {
            let values = segment.header.iter().filter_map(|part| match part {
                Part::Value(value) => Some(value.clone()),
                Part::Text(_) => None,
            });
            match source {
                Source::Values { values: source, .. } => {
                    if let Some(value) = values.into_iter().next() {
                        *source = value;
                    }
                }
                Source::Range {
                    low,
                    high: Some(high),
                    ..
                } => {
                    let mut values = values.into_iter();
                    if let (Some(bound_low), Some(bound_high)) = (values.next(), values.next()) {
                        *low = bound_low;
                        *high = bound_high;
                    }
                }
                Source::Range { .. } => {}
            }
        };
        if let [segment] = &pipeline.segments[..] {
            if let Some(source) = &mut pipeline.source {
                rebind(source, segment);
            }
            if let Some(reversible) = &mut pipeline.reversible {
                rebind(&mut reversible.source, segment);
            }
        }
        pipeline.outer.extend(names.iter().cloned());
        (code, pipeline, names)
    }

    /// The chain's statements with `sink` run on each item.
    fn render(pipeline: &Pipeline, sink: &origin::Fragment) -> origin::Fragment {
        let mut text = pipeline.setup.clone();
        text.push(pipeline.state.clone());
        for segment in &pipeline.segments {
            if !segment.guards.is_empty() {
                text.push_text(format!("if !({}) {{ ", segment.guards.join(" || ")));
            }
            text.push(header(&segment.header));
            text.push_text(" { ");
            text.push(segment.entry.clone());
            text.push(sink.clone());
            text.push_text(&segment.exit);
            text.push_text(" } ");
            if !segment.guards.is_empty() {
                text.push_text("} ");
            }
        }
        text
    }

    /// `{ setup; loops with sink; result }`.
    fn consume(
        &mut self,
        pipeline: &Pipeline,
        before: &str,
        sink: origin::Fragment,
        result: &str,
    ) -> origin::Fragment {
        let mut text = origin::Fragment::plain(format!("{{ {before} "));
        text.push(Self::render(pipeline, &sink));
        text.push_text(format!("{result} }}"));
        text
    }

    /// The array of the chain's items.
    fn materialized(&mut self, pipeline: &Pipeline) -> origin::Fragment {
        let out = self.temporary("items");
        let sink = origin::Fragment::plain(format!("{out}.push({}); ", pipeline.item));
        self.consume(pipeline, &format!("let {out} = [];"), sink, &out)
    }

    /// A discarded iterator (`v.iter().map(f);`) as Rust runs it: its
    /// sources and closures are evaluated, and no stage runs.
    pub(super) fn discarded(&mut self, value: &ParsedExpr) -> Option<origin::Fragment> {
        if !matches!(self.inference.resolve(&value.ty), Ty::Iter(..)) {
            return None;
        }
        let pipeline = self.pipelines.get(value.source.as_str())?.clone();
        let (code, _, _) = self.bound_pipeline(pipeline);
        let mut text = origin::Fragment::plain("{ ");
        text.push(value.prelude.clone());
        text.push(code);
        text.push_text("}");
        Some(text)
    }

    /// The chain of a `for` loop's iterator, when it is one loop; else
    /// `None`, and the loop walks its array.
    pub(super) fn for_pipeline(&mut self, value: &ParsedExpr) -> Option<(Pipeline, Ty)> {
        let (pipeline, item) = self.pipeline_of(value)?;
        if pipeline.stop.is_some() || pipeline.segments.len() != 1 {
            return None;
        }
        Some((pipeline, item))
    }

    /// The parts of a single-loop chain for `parse_for_body`.
    pub(super) fn for_parts(
        pipeline: Pipeline,
        binding: &str,
    ) -> (
        origin::Fragment,
        origin::Fragment,
        origin::Fragment,
        origin::Fragment,
    ) {
        let segment = pipeline.segments.into_iter().next().expect("one loop");
        let mut entry = segment.entry;
        entry.push_text(format!("let {binding} = {}; ", pipeline.item));
        let mut setup = pipeline.setup;
        setup.push(pipeline.state);
        (
            setup,
            header(&segment.header),
            entry,
            origin::Fragment::plain(segment.exit),
        )
    }

    /// Rust's `FromIterator<item>` for a `collect()` into `target`, once
    /// `target` is known: a `Vec` or a `VecDeque` of the items, a `String`
    /// of text or `char`s, a map or a set, a `Result` or an `Option` of
    /// items that are, each of a payload that is. Else rustc's E0277.
    pub(super) fn collected_into(
        &mut self,
        target: &Ty,
        item: &Ty,
        collect: Span,
    ) -> Result<(), Error> {
        let item = self.inference.resolve(item);
        let fits = match self.inference.resolve(target) {
            target @ Ty::Array { kind, .. } if kind.length().is_none() => {
                let collected = Ty::sequence(kind, item.clone());
                self.unifies(&target, &collected) && self.unify_type(&target, &collected)
            }
            // A map of `(key, value)` items, a set of the items.
            target @ Ty::Keyed(kind, _) => {
                let collected = match (kind.is_map(), &item) {
                    (true, Ty::Tuple(pair)) if pair.len() == 2 => {
                        Some(Ty::Keyed(kind, pair.clone()))
                    }
                    (true, _) => None,
                    (false, _) => Some(Ty::Keyed(kind, Box::new([item.clone()]))),
                };
                match collected {
                    Some(collected) if self.unifies(&target, &collected) => {
                        self.unify_type(&target, &collected);
                        let Ty::Keyed(_, arguments) = self.inference.resolve(&collected) else {
                            unreachable!("built as a map or set");
                        };
                        return self.require_key(kind, &arguments[0], collect);
                    }
                    _ => false,
                }
            }
            Ty::String => matches!(
                without_references(&item),
                Ty::Char | Ty::String | Ty::Str | Ty::Infer(_)
            ),
            Ty::Result(payload, error) => {
                let (inner, failure) = (self.inference.fresh(), self.inference.fresh());
                let shape = Ty::Result(Box::new(inner.clone()), Box::new(failure.clone()));
                if self.unifies(&shape, &item) && self.unifies(&error, &failure) {
                    self.unify_type(&shape, &item);
                    self.unify_type(&error, &failure);
                    return self.collected_into(&payload, &inner, collect);
                }
                false
            }
            Ty::Option(payload) => {
                let inner = self.inference.fresh();
                let shape = Ty::Option(Box::new(inner.clone()));
                if self.unifies(&shape, &item) {
                    self.unify_type(&shape, &item);
                    return self.collected_into(&payload, &inner, collect);
                }
                false
            }
            _ => true,
        };
        if fits {
            return Ok(());
        }
        Err(self.uncollectable(target, &item, collect))
    }

    /// rustc's E0277 for a `collect()` into `target` of items no
    /// `FromIterator` impl of it takes.
    fn uncollectable(&self, target: &Ty, item: &Ty, collect: Span) -> Error {
        Error::MissingCapability {
            owner: self.inference.resolve(target).name(&self.models),
            capability: format!(
                "FromIterator<{}>",
                self.inference.resolve(item).name(&self.models)
            )
            .into(),
            operation: collect,
            derive: false,
            required_by: None,
        }
    }

    /// An iterator method on `value`: an adaptor extends its chain, a
    /// consumer runs it.
    pub(super) fn parse_iterator_method(
        &mut self,
        value: ParsedExpr,
        member: ImmutableString,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let Some((mut pipeline, item)) = self.pipeline_of(&value) else {
            let fallback = self.unsupported("an iterator method on a value without a chain");
            return Err(self.pipeline_error(&value, fallback));
        };
        // Items with drop glue may only go on to an owner: a container, or
        // a closure's parameter.
        if !matches!(member.as_str(), "collect" | "for_each" | "map" | "fold")
            && self.needs_drop(&item, member_span)?
        {
            return Err(Error::UnsupportedNominalSyntax {
                construct: format!(
                    "`{member}` on an iterator of values whose type implements `Drop`: dropping \
                     them where Rust does is not supported in Rawr yet"
                )
                .into(),
                source: member_span,
            });
        }
        let reference = |ty: &Ty| Ty::Reference {
            mutable: false,
            target: Box::new(ty.clone()),
        };
        // `collect::<Vec<_>>()`, `sum::<i64>()`.
        let mut turbofish = None;
        if matches!(self.peek(), Token::DoubleColon)
            && matches!(self.peek_second(), Token::LessThan)
        {
            self.next();
            self.next();
            turbofish = Some(self.parse_local_type()?);
            self.expect_close_angle("'>' after turbofish type arguments")?;
        }
        let accumulator = self.inference.fresh();
        // `collect()` takes the type its `let` or caller expects.
        let collect_hint = self
            .collect_hint
            .as_ref()
            .filter(|(_, depth)| *depth == self.expression_stack.len())
            .map(|(ty, _)| ty.clone());
        let (mut arguments, end) =
            if member.as_str() == "fold" && matches!(self.peek(), Token::LeftParen) {
                self.parse_fold_arguments(&accumulator, &item)?
            } else {
                // A crate function named as an argument is the closure that
                // calls it, as for `Option::map` (T327): `.map(text)`.
                self.script_callables = true;
                let iterator = Ty::iter(item.clone());
                self.parse_std_arguments(&iterator, &member, "iterator method arguments")?
            };
        let expression = span(value.span.start(), end);
        let arity = |expected: usize, arguments: &[ParsedExpr]| {
            if arguments.len() == expected {
                Ok(())
            } else {
                Err(Error::argument_count(
                    crate::resolve::Callee::Method,
                    crate::resolve::Qualified("Iterator", &member).to_string(),
                    expected,
                    arguments.len(),
                    span(member_span.start(), end),
                ))
            }
        };
        let function = |inputs: Vec<Ty>, output: Ty| Ty::Fn {
            inputs: inputs.into_boxed_slice(),
            output: Box::new(output),
        };
        let x = pipeline.item.clone();
        let stop = Self::stop_text(&pipeline);
        let is_some =
            |this: &mut Self, value: &str| this.prelude_op(PreludeOp::IsSome, [value.into()]);
        let unwrap =
            |this: &mut Self, value: &str| this.prelude_op(PreludeOp::Unwrap, [value.into()]);
        let some = |this: &mut Self, value: &str| this.prelude_op(PreludeOp::Some, [value.into()]);
        let none = |this: &mut Self| this.prelude_op(PreludeOp::None, []);

        // Rust's `ExactSizeIterator` through the adaptors that keep it.
        pipeline.exact &= !matches!(
            member.as_str(),
            "filter"
                | "filter_map"
                | "take_while"
                | "skip_while"
                | "flat_map"
                | "flatten"
                | "chain"
        );
        // Adaptors: a new chain.
        match member.as_str() {
            "map" | "filter" | "filter_map" | "take_while" | "skip_while" | "inspect" => {
                arity(1, &arguments)?;
                let output = match member.as_str() {
                    "filter" | "take_while" | "skip_while" => Ty::Bool,
                    "inspect" => Ty::Unit,
                    "filter_map" => Ty::Option(Box::new(self.inference.fresh())),
                    _ => self.inference.fresh(),
                };
                let input = match member.as_str() {
                    "map" | "filter_map" => item.clone(),
                    _ => reference(&item),
                };
                self.coerce_expr(&function(vec![input], output.clone()), &mut arguments[0])?;
                let f = self.bind(&mut pipeline, "function", arguments[0].rendered_source());
                let next = self.temporary("item");
                let (open, item) = match member.as_str() {
                    "map" => (
                        origin::Fragment::plain(format!("let {next} = {f}.__rawr_call({x}); ")),
                        self.inference.resolve(&output),
                    ),
                    "filter" => (
                        origin::Fragment::plain(format!(
                            "if !{f}.__rawr_call({x}) {{ continue; }} "
                        )),
                        item,
                    ),
                    "take_while" => (
                        origin::Fragment::plain(format!("if !{f}.__rawr_call({x}) {stop} ")),
                        item,
                    ),
                    "skip_while" => {
                        let skipping = self.bind_state(&mut pipeline, "skipping", "true".into());
                        (
                            origin::Fragment::plain(format!(
                                "if {skipping} {{ if {f}.__rawr_call({x}) {{ continue; }} {skipping} = false; }} "
                            )),
                            item,
                        )
                    }
                    "inspect" => (
                        origin::Fragment::plain(format!("{f}.__rawr_call({x}); ")),
                        item,
                    ),
                    _ => {
                        let option = self.temporary("option");
                        let mut open = origin::Fragment::plain(format!(
                            "let {option} = {f}.__rawr_call({x}); if !"
                        ));
                        let test = is_some(self, &option);
                        open.push(test);
                        open.push_text(format!(" {{ continue; }} let {next} = "));
                        let value = unwrap(self, &option);
                        open.push(value);
                        open.push_text("; ");
                        let Ty::Option(inner) = self.inference.resolve(&output) else {
                            unreachable!("filter_map's closure returns an Option");
                        };
                        (open, *inner)
                    }
                };
                if matches!(member.as_str(), "map" | "filter_map") {
                    pipeline.item = next;
                }
                if member.as_str() == "take_while" || member.as_str() == "skip_while" {
                    Self::stage(&mut pipeline, &open, "");
                } else {
                    let dropping = matches!(member.as_str(), "filter" | "filter_map");
                    Self::item_stage(&mut pipeline, &open, dropping);
                }
                return Ok(self.adapted(pipeline, item, expression));
            }
            "enumerate" => {
                arity(0, &arguments)?;
                let count = self.bind_state(&mut pipeline, "count", "0".into());
                let next = self.temporary("item");
                let open = format!("let {next} = [{count}, {x}]; {count} += 1; ");
                pipeline.item = next;
                Self::stage(&mut pipeline, &open.into(), "");
                return Ok(self.adapted(
                    pipeline,
                    Ty::Tuple(Box::new([Ty::Int(Int::Usize), item])),
                    expression,
                ));
            }
            // A bare integer range steps from item to item, as Rust's
            // `StepBy` of a range does, never visiting those in between.
            "step_by"
                if let Some(Source::Range {
                    low,
                    high: Some(high),
                    inclusive,
                    char: false,
                }) = pipeline.source.clone()
                    && let Ty::Int(kind) = self.inference.resolve(&item) =>
            {
                arity(1, &arguments)?;
                self.coerce_expr(&Ty::Int(Int::Usize), &mut arguments[0])?;
                let flags =
                    i32::from(inclusive) | i32::from(!kind.signed() && kind.bits() >= 64) << 1;
                let op = if kind.wide() {
                    PreludeOp::WideStepBy
                } else {
                    PreludeOp::StepBy
                };
                let step = self.prelude_op(
                    op,
                    [
                        low,
                        high,
                        arguments[0].rendered_source(),
                        flags.to_string().into(),
                    ],
                );
                let stepped = self.bind(&mut pipeline, "steps", step);
                let segment = &mut pipeline.segments[0];
                segment.header = vec![Part::Text(format!("for {x} in {stepped}"))];
                pipeline.source = None;
                pipeline.reversible = None;
                return Ok(self.adapted(pipeline, item, expression));
            }
            // A bare array or `low..` steps through its indexes or values.
            "step_by"
                if matches!(
                    pipeline.source,
                    Some(Source::Values { array: true, .. } | Source::Range { high: None, .. })
                ) =>
            {
                arity(1, &arguments)?;
                self.coerce_expr(&Ty::Int(Int::Usize), &mut arguments[0])?;
                let step = self.bind(&mut pipeline, "step", arguments[0].rendered_source());
                pipeline.setup.push_text(format!(
                    "if {step} == 0 {{ __rawr_throw \"assertion failed: step != 0\"; }} "
                ));
                match pipeline.source.take() {
                    Some(Source::Values { values, .. }) => {
                        let values = self.bind(&mut pipeline, "values", values);
                        let index = self.temporary("index");
                        let flags = "0";
                        let steps = self.prelude_op(
                            PreludeOp::StepBy,
                            [
                                "0".into(),
                                format!("{values}.len()").into(),
                                step.as_str().into(),
                                flags.into(),
                            ],
                        );
                        let steps = self.bind(&mut pipeline, "steps", steps);
                        let segment = &mut pipeline.segments[0];
                        segment.header = vec![Part::Text(format!("for {index} in {steps}"))];
                        let mut entry =
                            origin::Fragment::plain(format!("let {x} = {values}[{index}]; "));
                        entry.push(std::mem::take(&mut segment.entry));
                        segment.entry = entry;
                    }
                    _ => {
                        // `let item = next; next += 1;` steps by `step`.
                        let segment = &mut pipeline.segments[0];
                        let entry = segment.entry.as_str().to_string();
                        let stepped = entry.replacen(" += 1; ", &format!(" += {step}; "), 1);
                        debug_assert_ne!(entry, stepped);
                        segment.entry = origin::Fragment::plain(stepped);
                    }
                }
                pipeline.reversible = None;
                return Ok(self.adapted(pipeline, item, expression));
            }
            "take" | "skip" | "step_by" => {
                arity(1, &arguments)?;
                self.coerce_expr(&Ty::Int(Int::Usize), &mut arguments[0])?;
                let n = self.bind(&mut pipeline, "count", arguments[0].rendered_source());
                let counter = self.bind_state(&mut pipeline, "counter", "0".into());
                let (open, close) = match member.as_str() {
                    "take" => (
                        format!("if {counter} >= {n} {stop} {counter} += 1; "),
                        format!("if {counter} >= {n} {stop} "),
                    ),
                    "skip" => (
                        format!("if {counter} < {n} {{ {counter} += 1; continue; }} "),
                        String::new(),
                    ),
                    _ => (
                        format!("{counter} += 1; if ({counter} - 1) % {n} != 0 {{ continue; }} "),
                        String::new(),
                    ),
                };
                Self::stage(&mut pipeline, &open.into(), &close);
                return Ok(self.adapted(pipeline, item, expression));
            }
            "cloned" | "copied" => {
                arity(0, &arguments)?;
                let item = match self.inference.resolve(&item) {
                    Ty::Reference { target, .. } => *target,
                    item => item,
                };
                return Ok(self.adapted(pipeline, item, expression));
            }
            // A kept iterator is the array of its items, whose first is
            // what `peek` sees.
            "into_iter" | "peekable" => {
                arity(0, &arguments)?;
                return Ok(self.adapted(pipeline, item, expression));
            }
            // A chain runs from its sources at each use, so its clone is
            // the chain itself. Rust's iterators are `Clone` when their
            // items are.
            "clone" => {
                arity(0, &arguments)?;
                if !self.type_provides(&item, capability::Kind::Clone) {
                    return Err(Error::MissingCapability {
                        owner: Ty::iter(item).name(&self.models),
                        capability: "Clone".into(),
                        operation: expression,
                        derive: false,
                        required_by: None,
                    });
                }
                return Ok(self.adapted(pipeline, item, expression));
            }
            "rev" => {
                arity(0, &arguments)?;
                let pipeline = self.reversed(pipeline);
                return Ok(self.adapted(pipeline, item, expression));
            }
            "zip" => {
                arity(1, &arguments)?;
                let Some((other, other_item)) = self.pipeline_of(&arguments[0]) else {
                    return Err(Error::TypeMismatch {
                        expected: "an iterator or a collection to `zip`".into(),
                        actual: arguments[0].ty.name(&self.models),
                        expression: arguments[0].span,
                    });
                };
                let next = self.temporary("item");
                let open = match other.source.clone() {
                    // A bare range counts: `v.iter().zip(1..)`.
                    Some(Source::Range {
                        low,
                        high,
                        inclusive,
                        char,
                    }) => {
                        let start = self.bind(&mut pipeline, "start", low);
                        let at = self.bind_state(&mut pipeline, "at", start.into());
                        let mut open = String::new();
                        if let Some(high) = high {
                            let high = self.bind(&mut pipeline, "end", high);
                            let operator = if inclusive { ">" } else { ">=" };
                            open += &format!("if {at} {operator} {high} {stop} ");
                        }
                        let other = self.range_item(char, &at);
                        open + &*format!("let {next} = [{x}, {other}]; {at} += 1; ")
                    }
                    _ => {
                        let values = self.materialized(&other);
                        let values = self.bind(&mut pipeline, "zipped", values);
                        let at = self.bind_state(&mut pipeline, "at", "0".into());
                        format!(
                            "if {at} >= {values}.len() {stop} let {next} = [{x}, {values}[{at}]]; {at} += 1; "
                        )
                    }
                };
                pipeline.item = next;
                Self::stage(&mut pipeline, &open.into(), "");
                return Ok(self.adapted(
                    pipeline,
                    Ty::Tuple(Box::new([item, other_item])),
                    expression,
                ));
            }
            "chain" => {
                arity(1, &arguments)?;
                let Some((mut other, other_item)) = self.pipeline_of(&arguments[0]) else {
                    return Err(Error::TypeMismatch {
                        expected: "an iterator or a collection to `chain`".into(),
                        actual: arguments[0].ty.name(&self.models),
                        expression: arguments[0].span,
                    });
                };
                self.require_type(&item, &other_item, arguments[0].span)?;
                let flag = self.nest(&mut pipeline);
                let next = self.temporary("item");
                for segment in &mut pipeline.segments {
                    segment
                        .entry
                        .push_text(format!("let {next} = {}; ", pipeline.item));
                }
                pipeline.setup.push(other.setup);
                pipeline.names.extend(other.names);
                pipeline.outer.extend(other.outer);
                pipeline.state.push(other.state);
                for (index, segment) in other.segments.iter_mut().enumerate() {
                    segment
                        .entry
                        .push_text(format!("let {next} = {}; ", other.item));
                    if index > 0
                        && let Some(inner) = &other.stop
                    {
                        segment.guards.push(inner.clone());
                    }
                    segment.guards.push(flag.clone());
                }
                pipeline.segments.extend(other.segments);
                pipeline.item = next;
                pipeline.source = None;
                pipeline.reversible = None;
                return Ok(self.adapted(pipeline, item, expression));
            }
            "flat_map" | "flatten" => {
                let f = if member.as_str() == "flat_map" {
                    arity(1, &arguments)?;
                    let output = self.inference.fresh();
                    let expected = function(vec![item.clone()], output.clone());
                    self.coerce_expr(&expected, &mut arguments[0])?;
                    let f = self.bind(&mut pipeline, "function", arguments[0].rendered_source());
                    Some((f, output))
                } else {
                    arity(0, &arguments)?;
                    None
                };
                let inner_ty = match &f {
                    Some((_, output)) => self.inference.resolve(output),
                    None => self.inference.resolve(&item),
                };
                let inner = match f {
                    Some((f, _)) => format!("{f}.__rawr_call({x})"),
                    None => x.clone(),
                };
                let next = self.temporary("item");
                let (open, close, item) = match inner_ty {
                    // An `Option` yields its value, if any.
                    Ty::Option(inner_item) => {
                        let option = self.temporary("option");
                        let mut open =
                            origin::Fragment::plain(format!("let {option} = {inner}; if !"));
                        let test = is_some(self, &option);
                        open.push(test);
                        open.push_text(format!(" {{ continue; }} let {next} = "));
                        let value = unwrap(self, &option);
                        open.push(value);
                        open.push_text("; ");
                        (open, String::new(), *inner_item)
                    }
                    inner_ty => {
                        let inner_item = match inner_ty {
                            Ty::Iter(item, _) => *item,
                            Ty::Array { element, .. } => *element,
                            Ty::InclusiveRange(kind) => Ty::Int(kind),
                            Ty::Reference { mutable, target } => match *target {
                                Ty::Array { element, .. } => Ty::Reference {
                                    mutable,
                                    target: element,
                                },
                                other => {
                                    return Err(Error::TypeMismatch {
                                        expected: "an iterator or a collection".into(),
                                        actual: other.name(&self.models),
                                        expression,
                                    })
                                }
                            },
                            other => {
                                return Err(Error::TypeMismatch {
                                    expected: "an iterator or a collection".into(),
                                    actual: other.name(&self.models),
                                    expression,
                                })
                            }
                        };
                        let flag = self.nest(&mut pipeline);
                        (
                            origin::Fragment::plain(format!("for {next} in ({inner}) {{ ")),
                            format!("}} if {flag} {{ break; }} "),
                            inner_item,
                        )
                    }
                };
                pipeline.item = next;
                Self::stage(&mut pipeline, &open, &close);
                return Ok(self.adapted(pipeline, item, expression));
            }
            _ => {}
        }

        // Consumers: a value.
        let (source, ty) = match member.as_str() {
            "collect" => {
                arity(0, &arguments)?;
                let target = turbofish
                    .or(collect_hint)
                    .map(|ty| self.inference.resolve(&ty));
                match target {
                    Some(Ty::String) => {
                        self.collected_into(&Ty::String, &item, member_span)?;
                        let out = self.temporary("text");
                        let sink = origin::Fragment::plain(format!("{out} += {x}; "));
                        let source =
                            self.consume(&pipeline, &format!("let {out} = \"\";"), sink, &out);
                        (source, Ty::String)
                    }
                    // A map of `(key, value)` items or a set, sorted by key.
                    Some(target @ Ty::Keyed(kind, _)) => {
                        let collected = if kind.is_map() {
                            let Ty::Tuple(pair) = self.inference.resolve(&item) else {
                                return Err(Error::TypeMismatch {
                                    expected: "`(key, value)` items to collect into a map".into(),
                                    actual: item.name(&self.models),
                                    expression,
                                });
                            };
                            if pair.len() != 2 {
                                return Err(Error::TypeMismatch {
                                    expected: "`(key, value)` items to collect into a map".into(),
                                    actual: item.name(&self.models),
                                    expression,
                                });
                            }
                            Ty::Keyed(kind, pair)
                        } else {
                            Ty::Keyed(kind, Box::new([item]))
                        };
                        self.require_type(&target, &collected, expression)?;
                        let Ty::Keyed(_, arguments) = self.inference.resolve(&collected) else {
                            unreachable!("built as a map or set");
                        };
                        self.require_key(kind, &arguments[0], expression)?;
                        let items = self.materialized(&pipeline);
                        let op = if kind.is_map() {
                            PreludeOp::MapFrom
                        } else {
                            PreludeOp::SetFrom
                        };
                        let helper = self.push_op(self.keyed_op(op, kind, &arguments[0]));
                        (origin::Fragment::call(&helper, [items]), collected)
                    }
                    // A crate type's `impl FromIterator<A>`: its
                    // `from_iter` of the items.
                    Some(target @ Ty::Script(id, _))
                        if let Some(definition) = self
                            .models
                            .std_impl(id, "FromIterator", "from_iter")
                            .cloned() =>
                    {
                        if let Some(argument) = definition.trait_arguments().first() {
                            self.require_type(&Ty::from_resolved(argument), &item, expression)?;
                        }
                        let items = self.materialized(&pipeline);
                        (
                            origin::Fragment::call(method_name(&definition), [items]),
                            target,
                        )
                    }
                    // `FromIterator` of a `Vec` or a `VecDeque`.
                    Some(target @ Ty::Array { kind, .. }) => {
                        let kind = match kind {
                            sequence::Kind::Deque => sequence::Kind::Deque,
                            _ => sequence::Kind::Vec,
                        };
                        let collected = Ty::sequence(kind, item.clone());
                        if !self.unifies(&target, &collected) {
                            return Err(self.uncollectable(&target, &item, member_span));
                        }
                        self.require_type(&target, &collected, expression)?;
                        (self.materialized(&pipeline), collected)
                    }
                    // A `Result` or an `Option`, which stops at the first
                    // `Err` or `None`, or a target inference decides: a
                    // `Vec`, or a `String` it concatenates
                    // (`-> String { s.chars().rev().collect() }`).
                    target => {
                        let target = match target {
                            Some(target @ (Ty::Result(..) | Ty::Option(_))) => target,
                            _ => self.inference.fresh(),
                        };
                        self.pending_collects
                            .push((target.clone(), item.clone(), member_span));
                        let test = self.push_op(Op::CollectStop {
                            target: target.clone(),
                        });
                        let helper = self.push_op(Op::Collect {
                            target: target.clone(),
                        });
                        let out = self.temporary("items");
                        let sink = origin::Fragment::plain(format!(
                            "{out}.push({x}); if {test}({x}) {stop} "
                        ));
                        let source = self.consume(
                            &pipeline,
                            &format!("let {out} = [];"),
                            sink,
                            &format!("{helper}({out})"),
                        );
                        (source, target)
                    }
                }
            }
            // `ExactSizeIterator::len`: the source's length when no stage
            // drops items, else the chain's count.
            "len" if pipeline.exact => {
                arity(0, &arguments)?;
                match pipeline.reversible.clone() {
                    Some(reversible) if !reversible.dropping => {
                        (Self::source_length(&reversible.source), Ty::Int(Int::Usize))
                    }
                    _ => {
                        let n = self.temporary("count");
                        let sink = origin::Fragment::plain(format!("{n} += 1; "));
                        (
                            self.consume(&pipeline, &format!("let {n} = 0;"), sink, &n),
                            Ty::Int(Int::Usize),
                        )
                    }
                }
            }
            // `(a..b).contains(&x)` of a bare range: its bounds compared,
            // a char by its code point.
            "contains"
                if let Some(Source::Range {
                    low,
                    high,
                    inclusive,
                    char,
                }) = pipeline.source.clone() =>
            {
                arity(1, &arguments)?;
                self.coerce_expr(&reference(&item), &mut arguments[0])?;
                let value = self.temporary("value");
                let mut source = origin::Fragment::plain(format!("{{ let {value} = "));
                if char {
                    let code = self.prelude_op(
                        super::PreludeOp::Cast,
                        [arguments[0].rendered_source(), "7".into()],
                    );
                    source.push(code);
                } else {
                    source.push(arguments[0].rendered_source());
                }
                source.push_text("; (");
                source.push(low);
                source.push_text(format!(") <= {value}"));
                if let Some(high) = high {
                    let operator = if inclusive { "<=" } else { "<" };
                    source.push_text(format!(" && {value} {operator} ("));
                    source.push(high);
                    source.push_text(")");
                }
                source.push_text(" }");
                (source, Ty::Bool)
            }
            "count" => {
                arity(0, &arguments)?;
                let n = self.temporary("count");
                let sink = origin::Fragment::plain(format!("{n} += 1; "));
                (
                    self.consume(&pipeline, &format!("let {n} = 0;"), sink, &n),
                    Ty::Int(Int::Usize),
                )
            }
            "sum" | "product" => {
                arity(0, &arguments)?;
                let mut total = match self.inference.resolve(&item) {
                    Ty::Reference { target, .. } => *target,
                    item => item,
                };
                if let Some(written) = turbofish {
                    self.require_type(&written, &total, expression)?;
                    total = self.inference.resolve(&written);
                }
                let (zero, one) = match total {
                    Ty::Int(_) => ("0", "1"),
                    Ty::Float(_) => ("0.0", "1.0"),
                    other => {
                        return Err(Error::TypeMismatch {
                            expected: "i64 or f64 items to sum".into(),
                            actual: other.name(&self.models),
                            expression,
                        })
                    }
                };
                let acc = self.temporary("total");
                let (start, operator) = if member.as_str() == "sum" {
                    (zero, "+")
                } else {
                    (one, "*")
                };
                // Each step as the item kind's own arithmetic: a `u64`'s
                // unsigned, a narrower kind's overflow panic, an `f32`
                // rounded.
                let step = match &total {
                    Ty::Int(Int::I64) | Ty::Float(Float::F64) => None,
                    Ty::Int(_) => Some(self.int_op(
                        operator,
                        acc.as_str().into(),
                        x.as_str().into(),
                        &total,
                        &total,
                    )),
                    Ty::Float(Float::F32) => Some(self.prelude_op(
                        PreludeOp::RoundF32,
                        [format!("{acc} {operator} {x}").into()],
                    )),
                    _ => None,
                };
                let sink = match step {
                    Some(step) => {
                        let mut sink = origin::Fragment::plain(format!("{acc} = "));
                        sink.push(step);
                        sink.push_text("; ");
                        sink
                    }
                    None => origin::Fragment::plain(format!("{acc} {operator}= {x}; ")),
                };
                let source = self.consume(&pipeline, &format!("let {acc} = {start};"), sink, &acc);
                (source, total)
            }
            "fold" => {
                arity(2, &arguments)?;
                let initial_ty = arguments[0].ty.clone();
                self.require_type(&accumulator, &initial_ty, arguments[0].span)?;
                let expected =
                    function(vec![accumulator.clone(), item.clone()], accumulator.clone());
                self.coerce_expr(&expected, &mut arguments[1])?;
                let acc = self.bind(&mut pipeline, "acc", arguments[0].rendered_source());
                let f = self.bind(&mut pipeline, "function", arguments[1].rendered_source());
                let sink =
                    origin::Fragment::plain(format!("{acc} = {f}.__rawr_call({acc}, {x}); "));
                (
                    self.consume(&pipeline, "", sink, &acc),
                    self.inference.resolve(&accumulator),
                )
            }
            "for_each" => {
                arity(1, &arguments)?;
                self.coerce_expr(&function(vec![item.clone()], Ty::Unit), &mut arguments[0])?;
                let f = self.bind(&mut pipeline, "function", arguments[0].rendered_source());
                let sink = origin::Fragment::plain(format!("{f}.__rawr_call({x}); "));
                (self.consume(&pipeline, "", sink, "()"), Ty::Unit)
            }
            "any" | "all" => {
                arity(1, &arguments)?;
                self.coerce_expr(&function(vec![item.clone()], Ty::Bool), &mut arguments[0])?;
                let f = self.bind(&mut pipeline, "function", arguments[0].rendered_source());
                let found = self.temporary("found");
                let (start, test, set) = if member.as_str() == "any" {
                    ("false", "", "true")
                } else {
                    ("true", "!", "false")
                };
                let sink = origin::Fragment::plain(format!(
                    "if {test}{f}.__rawr_call({x}) {{ {found} = {set}; {stop} }} "
                ));
                let source =
                    self.consume(&pipeline, &format!("let {found} = {start};"), sink, &found);
                (source, Ty::Bool)
            }
            "find" | "position" | "find_map" | "last" | "nth" | "next" | "peek" | "next_if"
            | "next_if_eq" => {
                let found = self.temporary("found");
                let mut sink = origin::Fragment::default();
                let ty = match member.as_str() {
                    "find" => {
                        arity(1, &arguments)?;
                        let expected = function(vec![reference(&item)], Ty::Bool);
                        self.coerce_expr(&expected, &mut arguments[0])?;
                        let f =
                            self.bind(&mut pipeline, "function", arguments[0].rendered_source());
                        sink.push_text(format!("if {f}.__rawr_call({x}) {{ {found} = "));
                        let value = some(self, &x);
                        sink.push(value);
                        sink.push_text(format!("; {stop} }} "));
                        Ty::Option(Box::new(item))
                    }
                    "position" => {
                        arity(1, &arguments)?;
                        self.coerce_expr(
                            &function(vec![item.clone()], Ty::Bool),
                            &mut arguments[0],
                        )?;
                        let f =
                            self.bind(&mut pipeline, "function", arguments[0].rendered_source());
                        let at = self.bind(&mut pipeline, "at", "0".into());
                        sink.push_text(format!("if {f}.__rawr_call({x}) {{ {found} = "));
                        let value = some(self, &at);
                        sink.push(value);
                        sink.push_text(format!("; {stop} }} {at} += 1; "));
                        Ty::Option(Box::new(Ty::Int(Int::Usize)))
                    }
                    "find_map" => {
                        arity(1, &arguments)?;
                        let output = Ty::Option(Box::new(self.inference.fresh()));
                        self.coerce_expr(
                            &function(vec![item.clone()], output.clone()),
                            &mut arguments[0],
                        )?;
                        let f =
                            self.bind(&mut pipeline, "function", arguments[0].rendered_source());
                        let option = self.temporary("option");
                        sink.push_text(format!("let {option} = {f}.__rawr_call({x}); if "));
                        let test = is_some(self, &option);
                        sink.push(test);
                        sink.push_text(format!(" {{ {found} = {option}; {stop} }} "));
                        self.inference.resolve(&output)
                    }
                    "nth" => {
                        arity(1, &arguments)?;
                        self.coerce_expr(&Ty::Int(Int::Usize), &mut arguments[0])?;
                        let n = self.bind(&mut pipeline, "count", arguments[0].rendered_source());
                        let at = self.bind(&mut pipeline, "at", "0".into());
                        sink.push_text(format!("if {at} == {n} {{ {found} = "));
                        let value = some(self, &x);
                        sink.push(value);
                        sink.push_text(format!("; {stop} }} {at} += 1; "));
                        Ty::Option(Box::new(item))
                    }
                    // `it.peek()`, `it.next_if(f)`, `it.next_if_eq(&x)` on a
                    // `let mut it`: its first item, taken only when it passes.
                    "peek" | "next_if" | "next_if_eq"
                        if let Some(Lvalue::Binding { binding, .. }) = &value.lvalue
                            && matches!(self.inference.resolve(&value.ty), Ty::Iter(..)) =>
                    {
                        let binding = binding.clone();
                        self.require_mutable_binding(&binding, value.span)?;
                        let first = format!("{binding}[0]");
                        let (test, ty) = match member.as_str() {
                            "peek" => {
                                arity(0, &arguments)?;
                                (None, Ty::Option(Box::new(reference(&item))))
                            }
                            "next_if" => {
                                arity(1, &arguments)?;
                                let expected = function(vec![reference(&item)], Ty::Bool);
                                self.coerce_expr(&expected, &mut arguments[0])?;
                                let mut test = arguments[0].rendered_source().surrounded("(", ")");
                                test.push_text(format!(".__rawr_call({first})"));
                                (Some(test), Ty::Option(Box::new(item)))
                            }
                            _ => {
                                arity(1, &arguments)?;
                                self.coerce_expr(&reference(&item), &mut arguments[0])?;
                                let test = self.equal_source(
                                    &item,
                                    origin::Fragment::plain(first.clone()),
                                    arguments[0].rendered_source(),
                                    expression,
                                )?;
                                (Some(test), Ty::Option(Box::new(item)))
                            }
                        };
                        let mut source = origin::Fragment::plain(format!("if {binding}.len() > 0"));
                        if let Some(test) = test {
                            source.push_text(" && ");
                            source.push(test);
                        }
                        source.push_text(" { ");
                        let taken = if member.as_str() == "peek" {
                            first
                        } else {
                            format!("{binding}.shift()")
                        };
                        let found = some(self, &taken);
                        source.push(found);
                        source.push_text(" } else { ");
                        let empty = none(self);
                        source.push(empty);
                        source.push_text(" }");
                        return Ok(self.operator_expr(source.surrounded("(", ")"), ty, expression));
                    }
                    "peek" => {
                        arity(0, &arguments)?;
                        sink.push_text(format!("{found} = "));
                        let value = some(self, &x);
                        sink.push(value);
                        sink.push_text(format!("; {stop} "));
                        Ty::Option(Box::new(reference(&item)))
                    }
                    "next" => {
                        arity(0, &arguments)?;
                        // `it.next()` on a `let mut it`: its first item,
                        // taken from its array.
                        if let Some(Lvalue::Binding { binding, .. }) = &value.lvalue
                            && matches!(self.inference.resolve(&value.ty), Ty::Iter(..))
                        {
                            let Some(local) = self.bindings.get(binding) else {
                                return Err(
                                    self.unsupported("assignment through an unknown binding")
                                );
                            };
                            if !local.mutable {
                                return Err(Error::borrow_immutable(binding, value.span));
                            }
                            self.mutable_use(&value, value.span);
                            let mut source =
                                origin::Fragment::plain(format!("if {binding}.len() == 0 {{ "));
                            let empty = none(self);
                            source.push(empty);
                            source.push_text(" } else { ");
                            let first = some(self, &format!("{binding}.shift()"));
                            source.push(first);
                            source.push_text(" }");
                            let ty = Ty::Option(Box::new(item));
                            return Ok(self.operator_expr(
                                source.surrounded("(", ")"),
                                ty,
                                expression,
                            ));
                        }
                        // `self.items.next()` on a field, an element or a
                        // nested place holding a `Box<dyn Iterator>`: its
                        // first item, taken from the array the place holds.
                        if matches!(
                            &value.lvalue,
                            Some(Lvalue::Field(_) | Lvalue::Nested(_) | Lvalue::Place { .. })
                        ) && matches!(self.inference.resolve(&value.ty), Ty::Iter(..))
                        {
                            let shifted = self.mutation_call(&value, "shift", Vec::new())?;
                            let mut source = origin::Fragment::plain("if ");
                            source.push(value.rendered_source().surrounded("(", ")"));
                            source.push_text(".len() == 0 { ");
                            let empty = none(self);
                            source.push(empty);
                            source.push_text(" } else { ");
                            let first = self.prelude_op(PreludeOp::Some, [shifted]);
                            source.push(first);
                            source.push_text(" }");
                            let ty = Ty::Option(Box::new(item));
                            return Ok(self.operator_expr(
                                source.surrounded("(", ")"),
                                ty,
                                expression,
                            ));
                        }
                        sink.push_text(format!("{found} = "));
                        let value = some(self, &x);
                        sink.push(value);
                        sink.push_text(format!("; {stop} "));
                        Ty::Option(Box::new(item))
                    }
                    _ => {
                        arity(0, &arguments)?;
                        sink.push_text(format!("{found} = "));
                        let value = some(self, &x);
                        sink.push(value);
                        sink.push_text("; ");
                        Ty::Option(Box::new(item))
                    }
                };
                let mut before = origin::Fragment::plain(format!("let {found} = "));
                let empty = none(self);
                before.push(empty);
                before.push_text(";");
                let mut source = origin::Fragment::plain("{ ");
                source.push(before);
                source.push_text(" ");
                source.push(Self::render(&pipeline, &sink));
                source.push_text(format!("{found} }}"));
                (source, ty)
            }
            // `(Vec<A>, Vec<B>)` of `(A, B)` items.
            "unzip" => {
                arity(0, &arguments)?;
                let Ty::Tuple(pair) = self.inference.resolve(&item) else {
                    return Err(Error::TypeMismatch {
                        expected: "`(A, B)` items to `unzip`".into(),
                        actual: item.name(&self.models),
                        expression,
                    });
                };
                let [left, right] = &pair[..] else {
                    return Err(Error::TypeMismatch {
                        expected: "`(A, B)` items to `unzip`".into(),
                        actual: item.name(&self.models),
                        expression,
                    });
                };
                let (lefts, rights) = (self.temporary("lefts"), self.temporary("rights"));
                let sink = origin::Fragment::plain(format!(
                    "{lefts}.push({x}[0]); {rights}.push({x}[1]); "
                ));
                let vec = |element: &Ty| Ty::sequence(sequence::Kind::Vec, element.clone());
                let source = self.consume(
                    &pipeline,
                    &format!("let {lefts} = []; let {rights} = [];"),
                    sink,
                    &format!("[{lefts}, {rights}]"),
                );
                (source, Ty::Tuple(Box::new([vec(left), vec(right)])))
            }
            "partition" => {
                arity(1, &arguments)?;
                self.coerce_expr(
                    &function(vec![reference(&item)], Ty::Bool),
                    &mut arguments[0],
                )?;
                let f = self.bind(&mut pipeline, "function", arguments[0].rendered_source());
                let (yes, no) = (self.temporary("yes"), self.temporary("no"));
                let sink = origin::Fragment::plain(format!(
                    "if {f}.__rawr_call({x}) {{ {yes}.push({x}); }} else {{ {no}.push({x}); }} "
                ));
                let vec = Ty::sequence(sequence::Kind::Vec, item);
                let source = self.consume(
                    &pipeline,
                    &format!("let {yes} = []; let {no} = [];"),
                    sink,
                    &format!("[{yes}, {no}]"),
                );
                (source, Ty::Tuple(Box::new([vec.clone(), vec])))
            }
            "min" | "max" | "min_by_key" | "max_by_key" | "min_by" | "max_by" => {
                // Rust's first minimum and last maximum, over the items.
                let by = match (member.as_str(), arguments.as_mut_slice()) {
                    ("min" | "max", []) => None,
                    ("min" | "max", _) => return Err(arity(0, &arguments).unwrap_err()),
                    (_, [by]) => Some(by),
                    _ => return Err(arity(1, &arguments).unwrap_err()),
                };
                let items = self.materialized(&pipeline);
                let (element, borrowed) = match self.inference.resolve(&item) {
                    Ty::Reference { target, .. } => (*target, true),
                    item => (item, false),
                };
                let input = reference(&self.inference.resolve(&item));
                let by = by.map(|by| (by, input));
                let (source, best) = self.extremum(items, &element, &member, by, member_span)?;
                let best = if borrowed { best } else { element };
                (source, Ty::Option(Box::new(best)))
            }
            // `Chars::as_str`: the text of the chars left, which the
            // iterator borrows from.
            "as_str"
                if self.inference.resolve(&value.ty)
                    == Ty::Iter(Box::new(Ty::Char), Some(CHARS)) =>
            {
                arity(0, &arguments)?;
                let out = self.temporary("text");
                let sink = origin::Fragment::plain(format!("{out} += {x}; "));
                let source = self.consume(&pipeline, &format!("let {out} = \"\";"), sink, &out);
                (source, Ty::Str)
            }
            // A method std declares that rawr doesn't run yet (D712.1):
            // a std iterator struct's own, and `ExactSizeIterator`'s only
            // while the chain is one.
            _ => {
                if let Some(found) =
                    std_method::inherent(&self.inference.resolve(&value.ty), &member)
                    && !std_method::ITERATOR.contains(&found.owner)
                {
                    return Err(Error::NotYet {
                        path: std_method::path(&found).into(),
                        reference: member_span,
                    });
                }
                let exact = pipeline.exact;
                if let Some(found) = std_method::iterator(&member, |trait_| {
                    std_method::trait_name(trait_) != "ExactSizeIterator" || exact
                }) {
                    return Err(Error::NotYet {
                        path: std_method::path(&found).into(),
                        reference: member_span,
                    });
                }
                return Err(Error::UnknownMember {
                    owner: Ty::iter(item).name(&self.models),
                    member,
                    reference: member_span,
                });
            }
        };
        Ok(self.operator_expr(source.surrounded("(", ")"), ty, expression))
    }

    /// The number of values a source walks.
    fn source_length(source: &Source) -> origin::Fragment {
        match source {
            Source::Values { values, .. } => values.clone().surrounded("(", ").len()"),
            Source::Range {
                low,
                high: Some(high),
                inclusive,
                ..
            } => {
                let mut length = origin::Fragment::plain("{ let __rawr_length = (");
                length.push(high.clone());
                length.push_text(") - (");
                length.push(low.clone());
                length.push_text(if *inclusive { ") + 1;" } else { ");" });
                length.push_text(" if __rawr_length < 0 { 0 } else { __rawr_length } }");
                length
            }
            Source::Range { .. } => origin::Fragment::plain("0"),
        }
    }

    /// `(init, |acc, x| ..)`: the closure's accumulator takes `init`'s type,
    /// as Rust infers it.
    fn parse_fold_arguments(
        &mut self,
        accumulator: &Ty,
        item: &Ty,
    ) -> Result<(Vec<ParsedExpr>, Position), Error> {
        self.next();
        let initial = self.parse_expr()?;
        self.require_type(accumulator, &initial.ty, initial.span)?;
        self.expect(Token::Comma, "',' after `fold`'s initial value")?;
        self.closure_hint = Some(Box::new([accumulator.clone(), item.clone()]));
        let function = self.parse_expr();
        self.closure_hint = None;
        let function = function?;
        if matches!(self.peek(), Token::Comma) {
            self.next();
        }
        let end = self.expect(Token::RightParen, "')' after `fold`'s arguments")?;
        Ok((vec![initial, function], end))
    }

    fn adapted(&mut self, pipeline: Pipeline, item: Ty, expression: Span) -> ParsedExpr {
        let item = self.inference.resolve(&item);
        self.iterator_expr(pipeline, item, expression)
    }

    /// `rev()`: a source walked backwards, with the stages after it that
    /// map or drop items one by one (`v.iter().map(f).rev()` calls `f` from
    /// the last item, as Rust's does); any other chain, its array walked
    /// backwards.
    fn reversed(&mut self, pipeline: Pipeline) -> Pipeline {
        let Some(reversible) = pipeline.reversible.clone() else {
            let item = self.temporary("item");
            let mut reversed = self.walked_backwards(None, &pipeline, item);
            reversed.exact = pipeline.exact;
            reversed.outer = pipeline.outer;
            return reversed;
        };
        let mut reversed =
            self.walked_backwards(Some(reversible.source), &pipeline, reversible.item.clone());
        let mut setup = pipeline.setup;
        setup.push(reversed.setup);
        reversed.setup = setup;
        let mut names = pipeline.names;
        names.append(&mut reversed.names);
        reversed.names = names;
        let mut state = pipeline.state;
        state.push(reversed.state);
        reversed.state = state;
        let segment = &mut reversed.segments[0];
        segment.entry.push(reversible.entry);
        segment.exit = reversible.exit;
        reversed.item = pipeline.item;
        reversed.exact = pipeline.exact;
        reversed.outer = pipeline.outer;
        reversed
    }

    /// A loop over `source`'s values from the last, each in `item`; without
    /// a source, over the array of `pipeline`'s items.
    fn walked_backwards(
        &mut self,
        source: Option<Source>,
        pipeline: &Pipeline,
        item: String,
    ) -> Pipeline {
        let mut reversed = Pipeline {
            setup: origin::Fragment::default(),
            names: Vec::new(),
            outer: Vec::new(),
            state: origin::Fragment::default(),
            segments: Vec::new(),
            item: item.clone(),
            stop: None,
            source: None,
            reversible: None,
            exact: false,
        };
        let (header, entry) = match source {
            Some(Source::Range {
                low,
                high: Some(high),
                inclusive,
                char,
            }) => {
                let floor = self.bind(&mut reversed, "low", low);
                let mut top = high.surrounded("(", ")");
                if inclusive {
                    top.push_text(" + 1");
                }
                let top = self.bind(&mut reversed, "high", top);
                let at = self.bind_state(&mut reversed, "at", top.into());
                (
                    format!("while {at} > {floor}"),
                    format!("{at} -= 1; let {item} = {}; ", self.range_item(char, &at)),
                )
            }
            source => {
                let values = match source {
                    Some(Source::Values {
                        values,
                        array: true,
                    }) => values,
                    // A string's chars, as an array.
                    Some(source @ Source::Values { .. }) => {
                        let bare = self.source_pipeline(source);
                        self.materialized(&bare)
                    }
                    _ => self.materialized(pipeline),
                };
                let all = self.bind(&mut reversed, "values", values);
                let at = self.bind_state(&mut reversed, "at", format!("{all}.len()").into());
                (
                    format!("while {at} > 0"),
                    format!("{at} -= 1; let {item} = {all}[{at}]; "),
                )
            }
        };
        reversed.segments.push(Segment {
            guards: Vec::new(),
            header: vec![Part::Text(header)],
            entry: entry.into(),
            exit: String::new(),
        });
        reversed
    }
}

impl<'a> Parser<'a> {
    /// The item of the crate iterator `ty`: its `next`'s `Option<T>`'s `T`,
    /// as `Product<I, J>`'s `next` gives `(I::Item, J::Item)`. `None` when
    /// `ty` has no `next` Rawr has.
    #[cfg(feature = "rust-import")]
    pub(super) fn host_iterator_item(&mut self, ty: &Ty, span: Span) -> Option<Ty> {
        if !self.type_implements(ty, "Iterator") {
            return None;
        }
        let definition = ty
            .host_alias()
            .cloned()
            .or_else(|| self.imported_member_owner(ty, "next"))?;
        let next = self
            .models
            .host_items(&format!("{}::next", definition.path()))?
            .first()?
            .clone();
        let checkpoint = self.inference.checkpoint();
        let Ok(signature) = self.instantiate_host_signature(&next, Some(ty), None, span) else {
            self.inference.rollback(checkpoint);
            return None;
        };
        self.inference.commit(checkpoint);
        match self.inference.resolve(&signature.output) {
            Ty::Option(item) => Some(*item),
            _ => None,
        }
    }

    /// Whether `ty` is an iterator: the program's own (`program_iterator`)
    /// or a crate type that implements `Iterator`.
    #[cfg(feature = "rust-import")]
    pub(super) fn iterator_value(&self, ty: &Ty) -> bool {
        let ty = self.inference.resolve(ty);
        self.program_iterator(&ty)
            || matches!(ty, Ty::Host { .. }) && self.type_implements(&ty, "Iterator")
    }

    /// Whether `ty` is an iterator of the program's own, which crosses into
    /// a crate as a step function (`step_iterator`): an adaptor chain, an
    /// integer range, or a script type that implements `Iterator`.
    #[cfg(feature = "rust-import")]
    pub(super) fn program_iterator(&self, ty: &Ty) -> bool {
        match self.inference.resolve(ty) {
            Ty::Iter(..) | Ty::InclusiveRange(_) => true,
            ty @ Ty::Script(..) => self.type_implements(&ty, "Iterator"),
            _ => false,
        }
    }
}

/// `CollectStop`'s and `CollectTry`'s code for a `Result` target.
pub(super) const COLLECT_RESULT: INT = 1;
/// Their code for an `Option` target.
pub(super) const COLLECT_OPTION: INT = 2;

/// `collect()` into a `Result` or an `Option` (`kinds[0]`) of `items`, which
/// end at the first `Err` or `None` (`CollectStop`): that one, else `Ok` or
/// `Some` of the payloads, concatenated when the payload's kind
/// (`kinds[1]`) is a `String`'s.
pub(super) fn collect_try(kinds: [INT; 2], items: Dynamic) -> RawrResult {
    let items = items
        .into_array()
        .map_err(|_| invariant("collected items are not an array"))?;
    let mut payloads = crate::Array::with_capacity(items.len());
    for item in items {
        let payload = if kinds[0] == COLLECT_RESULT {
            match crate::result::read(&item) {
                Some(Ok(payload)) => payload,
                Some(Err(_)) => return Ok(item),
                None => return Err(invariant("a collected item is not a `Result`")),
            }
        } else {
            match crate::option::read(&item) {
                Some(Some(payload)) => payload,
                Some(None) => return Ok(item),
                None => return Err(invariant("a collected item is not an `Option`")),
            }
        };
        payloads.push(payload);
    }
    let collected = if kinds[1] == kind_code(Ty::String) {
        let mut text = String::new();
        for payload in &payloads {
            match payload.as_char() {
                Ok(character) => text.push(character),
                Err(_) => text.push_str(&payload.to_string()),
            }
        }
        Dynamic::from(ImmutableString::from(text))
    } else {
        Dynamic::from_array(payloads)
    };
    Ok(if kinds[0] == COLLECT_RESULT {
        crate::result::ok(collected)
    } else {
        crate::option::some(collected)
    })
}
