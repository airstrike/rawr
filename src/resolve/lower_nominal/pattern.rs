//! Patterns and the bindings they make: their syntax, and patterns resolved
//! against script and crate types.

use super::coverage;
use super::coverage::{
    field_order, reject_empty_range, render_literal, scalar_text, tuple_members, tuple_rest,
};
use super::lvalue::Lvalue;
use super::models::Nominal;
#[cfg(feature = "rust-import")]
use super::op::ImportedOperation;
use super::op::{ordering_discriminant, PreludeOp};
use super::places::PlacePattern;
use super::ty::{same_inferred, Int, Ty};
use super::{
    borrows, drops, instantiate, origin, pattern_name, point_span, sequence, span, text_end,
    Binding, BindingKind, Branch, Continuation, ParsedExpr, Parser,
};
use crate::ast::{NominalEnumPattern, NominalPattern, NominalPayloadType};
#[cfg(feature = "rust-import")]
use crate::capability;
use crate::resolve::borrowck;
use crate::resolve::borrowck::record::Matched;
use crate::resolve::{type_ref, BindingSite, Error};
use crate::tokenizer::Token;
use crate::types::nominal;
use crate::{ast, receiver, type_def, ImmutableString, Position, Span};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

#[derive(Clone)]
pub(super) enum Syntax {
    /// `name` or `mut name`.
    Binding {
        name: ImmutableString,
        span: Span,
        mutable: bool,
    },
    /// `ref name` or `ref mut name`: a reference to the matched value.
    Ref {
        name: ImmutableString,
        span: Span,
        mutable: bool,
    },
    Discard {
        span: Span,
    },
    /// `S` of `struct S;`: matches its one value.
    UnitStruct {
        ty: Ty,
        span: Span,
    },
    /// `W(a, b)` of `struct W(A, B);`: `ty` instantiated, `fields` its
    /// field types under that instance.
    TupleStruct {
        ty: Ty,
        definition: type_def::Definition,
        fields: Box<[Ty]>,
        members: Box<[Syntax]>,
        span: Span,
    },
    Enum(EnumSyntax),
    /// `Some(pattern)` or `None` on an `Option<T>`, `Ok(pattern)` or
    /// `Err(pattern)` on a `Result<T, E>`.
    Prelude {
        variant: Prelude,
        payload: Option<Box<Syntax>>,
        span: Span,
    },
    /// `Cow::Borrowed(pattern)` or `Cow::Owned(pattern)` (`owned`) on a
    /// `Cow<'_, B>`.
    Cow {
        owned: bool,
        payload: Box<Syntax>,
        span: Span,
    },
    /// `0`, `-1`, `'a'`, `"x"`, `true`, `1.5`.
    Literal {
        literal: ast::NominalLiteral,
        ty: Ty,
        span: Span,
    },
    /// `1..=9`, `'a'..='z'`, `0..10`, `5..`, `..=3`.
    Range {
        low: Option<(ast::NominalLiteral, Ty)>,
        high: Option<(ast::NominalLiteral, Ty)>,
        inclusive: bool,
        span: Span,
    },
    /// `(a, b)`.
    Tuple {
        members: Box<[Syntax]>,
        span: Span,
    },
    /// `p | q`.
    Or {
        alternatives: Box<[Syntax]>,
        span: Span,
    },
    /// `[a, b, rest @ .., z]` on an array or slice; `rest` is the `..`
    /// element (`Rest`) when written.
    Slice {
        before: Box<[Syntax]>,
        rest: Option<Box<Syntax>>,
        after: Box<[Syntax]>,
        span: Span,
    },
    /// `..` or `name @ ..`, an element of a slice or tuple pattern.
    Rest {
        binding: Option<(ImmutableString, Span)>,
        span: Span,
    },
    /// `&p` or `&mut p`.
    Reference {
        mutable: bool,
        pattern: Box<Syntax>,
        span: Span,
    },
    /// `name @ pattern`.
    At {
        name: ImmutableString,
        name_span: Span,
        pattern: Box<Syntax>,
    },
}

/// A constructor of Rust's prelude enums `Option<T>` and
/// `Result<T, E>`, builtin generics until Rawr has its own.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Prelude {
    Some,
    None,
    Ok,
    Err,
}

impl Prelude {
    pub(super) fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "Some" => Self::Some,
            "None" => Self::None,
            "Ok" => Self::Ok,
            "Err" => Self::Err,
            _ => return None,
        })
    }

    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Some => "Some",
            Self::None => "None",
            Self::Ok => "Ok",
            Self::Err => "Err",
        }
    }

    /// The enum that owns this constructor.
    pub(super) const fn owner(self) -> &'static str {
        match self {
            Self::Some | Self::None => "Option",
            Self::Ok | Self::Err => "Result",
        }
    }
}

impl Syntax {
    pub(super) fn span(&self) -> Span {
        match self {
            Self::Binding { span, .. }
            | Self::Ref { span, .. }
            | Self::Discard { span }
            | Self::UnitStruct { span, .. }
            | Self::TupleStruct { span, .. }
            | Self::Prelude { span, .. }
            | Self::Cow { span, .. }
            | Self::Literal { span, .. }
            | Self::Range { span, .. }
            | Self::Tuple { span, .. }
            | Self::Or { span, .. }
            | Self::Slice { span, .. }
            | Self::Rest { span, .. }
            | Self::Reference { span, .. } => *span,
            Self::Enum(pattern) => Span::new(
                pattern.owner_span.start(),
                text_end(pattern.variant_span.start(), &pattern.variant),
            ),
            Self::At {
                name_span, pattern, ..
            } => Span::new(name_span.start(), pattern.span().end()),
        }
    }

    /// The pattern of a value `witness` matches at the part written at
    /// `at` (rustc's witness), every other part `_`; `None` when no part
    /// of the pattern is at `at`.
    pub(super) fn witness_at(&self, at: Span, witness: &str) -> Option<String> {
        if self.span() == at {
            return Some(witness.to_string());
        }
        let members = |members: &[Self]| {
            let mut found = false;
            let rendered = members
                .iter()
                .map(|member| match member.witness_at(at, witness) {
                    Some(placed) => {
                        found = true;
                        placed
                    }
                    None if matches!(member, Self::Rest { .. }) => "..".to_string(),
                    None => "_".to_string(),
                })
                .collect::<Vec<_>>()
                .join(", ");
            found.then_some(rendered)
        };
        match self {
            Self::Tuple { members: parts, .. } => Some(format!("({})", members(parts)?)),
            Self::TupleStruct {
                definition,
                members: parts,
                ..
            } => Some(format!("{}({})", definition.name(), members(parts)?)),
            Self::Enum(pattern) => {
                let owner = pattern
                    .owner
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect::<Vec<_>>()
                    .join("::");
                let path = if pattern.host_struct {
                    owner
                } else {
                    format!("{owner}::{}", pattern.variant)
                };
                match &pattern.named {
                    Some((names, _)) => {
                        names
                            .iter()
                            .zip(pattern.payload.iter())
                            .find_map(|((name, _), member)| {
                                let placed = member.witness_at(at, witness)?;
                                Some(format!("{path} {{ {name}: {placed}, .. }}"))
                            })
                    }
                    None => Some(format!("{path}({})", members(&pattern.payload)?)),
                }
            }
            Self::Prelude {
                variant,
                payload: Some(payload),
                ..
            } => Some(format!(
                "{}({})",
                variant.name(),
                payload.witness_at(at, witness)?
            )),
            Self::Cow { owned, payload, .. } => Some(format!(
                "Cow::{}({})",
                if *owned { "Owned" } else { "Borrowed" },
                payload.witness_at(at, witness)?
            )),
            Self::Reference {
                mutable, pattern, ..
            } => Some(format!(
                "{}{}",
                if *mutable { "&mut " } else { "&" },
                pattern.witness_at(at, witness)?
            )),
            Self::At { pattern, .. } => pattern.witness_at(at, witness),
            _ => None,
        }
    }

    /// Whether a `ref mut` binding is in the pattern.
    pub(super) fn has_ref_mut(&self) -> bool {
        match self {
            Self::Ref { mutable, .. } => *mutable,
            Self::TupleStruct { members, .. } | Self::Tuple { members, .. } => {
                members.iter().any(Self::has_ref_mut)
            }
            Self::Slice { before, after, .. } => {
                before.iter().chain(after.iter()).any(Self::has_ref_mut)
            }
            Self::Reference { pattern, .. } | Self::At { pattern, .. } => pattern.has_ref_mut(),
            Self::Cow { payload, .. } => payload.has_ref_mut(),
            _ => false,
        }
    }

    /// Whether a part of the pattern may miss a value: a script enum's
    /// variant, a prelude or `Cow` variant, a literal, a range or an
    /// or-pattern. A `let` of one is irrefutable only when the coverage
    /// check finds it covers every value, as `let One(x)` of a one-variant
    /// enum does, and rustc's E0005 otherwise.
    pub(super) fn may_refute(&self) -> bool {
        match self {
            Self::Enum(pattern) => {
                !pattern.host_struct || pattern.payload.iter().any(Self::may_refute)
            }
            Self::Prelude { .. }
            | Self::Cow { .. }
            | Self::Literal { .. }
            | Self::Range { .. }
            | Self::Or { .. } => true,
            Self::TupleStruct { members, .. } | Self::Tuple { members, .. } => {
                members.iter().any(Self::may_refute)
            }
            Self::Slice { before, after, .. } => {
                before.iter().chain(after.iter()).any(Self::may_refute)
            }
            Self::Reference { pattern, .. } | Self::At { pattern, .. } => pattern.may_refute(),
            _ => false,
        }
    }

    /// `_` or a plain binding, which matches anything.
    pub(super) const fn is_irrefutable_leaf(&self) -> bool {
        matches!(
            self,
            Self::Binding { .. }
                | Self::Ref { .. }
                | Self::Discard { .. }
                | Self::UnitStruct { .. }
        )
    }
}

/// Names, each with the span it was written at.
pub(super) type SpannedNames = Box<[(ImmutableString, Span)]>;

#[derive(Clone)]
pub(super) struct EnumSyntax {
    pub(super) owner: SpannedNames,
    pub(super) owner_span: Span,
    pub(super) variant: ImmutableString,
    pub(super) variant_span: Span,
    pub(super) payload: Box<[Syntax]>,
    /// `V { x, y: p, .. }`: the field each of `payload` names, in written
    /// order, and whether `..` ends it.
    pub(super) named: Option<(SpannedNames, bool)>,
    /// `Point { x, y }` on a facade struct: `owner` is the whole path and
    /// `variant` its last name.
    pub(super) host_struct: bool,
}

/// The statement a pattern that must match every value binds in, which
/// names it in rustc's E0005.
#[derive(Clone, Copy)]
pub(in crate::resolve) enum Site {
    /// `let pattern = value;`.
    Let,
    /// `for pattern in values`.
    For,
    /// A function's parameter, `fn f(E::A(x): E)`.
    Argument,
}

pub(super) struct ResolvedEnum {
    pub(super) proof: NominalEnumPattern,
    pub(super) coverage: coverage::Pattern,
}

/// One name a pattern binds.
#[derive(Clone)]
pub(super) struct PatternBinding {
    pub(super) name: ImmutableString,
    pub(super) span: Span,
    pub(super) ty: Ty,
    pub(super) mutable: bool,
}

/// A pattern on a facade value, lowered to the facade's variant tests and
/// readers: the tests, in order, and the names it binds.
#[derive(Default)]
pub(super) struct Host {
    pub(super) tests: Vec<origin::Fragment>,
    pub(super) binds: Vec<PatternBinding>,
    pub(super) values: Vec<origin::Fragment>,
    /// The fields of crate values that bindings move or lend, each read
    /// once for every binding of its variant, and the fields that patterns
    /// below a variant's read.
    pub(super) parts: Vec<HostPart>,
    /// Each binding that reads a part: its index in `binds`, its part's
    /// index in `parts`, and the copy it reads where the part is not read.
    pub(super) readers: Vec<(usize, usize, origin::Fragment)>,
    /// The bindings and parts in the order they are bound: a binding of a
    /// whole value before a part that moves or lends it, and a part before
    /// what reads it.
    pub(super) steps: Vec<HostStep>,
}

/// One binding or part of a `Host`, by its index.
#[derive(Clone, Copy)]
pub(super) enum HostStep {
    Bind(usize),
    #[cfg_attr(
        not(feature = "rust-import"),
        expect(dead_code, reason = "only imported variants have parts")
    )]
    Part(usize),
}

/// Where a `Host`'s bindings are read.
#[derive(Clone, Copy)]
pub(super) enum HostReading {
    /// In the arm, the scope they are used in.
    Arm,
    /// In an arm's guard, which binds them by reference, as Rust does, so
    /// no part is moved before the arm is chosen.
    Guard,
    /// As values that leave the scope they are made in, as a `let`-`else`
    /// gives them to the statements after it, past the end of any lend.
    Escaping,
}

/// A crate value's fields, moved or lent for the bindings of one variant
/// pattern, or one field a pattern below the variant's reads, under a
/// hidden name.
pub(super) struct HostPart {
    pub(super) name: String,
    pub(super) reader: origin::Fragment,
    /// The reader where a guard reads the part, when it is not `reader`:
    /// one that lends what `reader` moves.
    pub(super) guard: Option<origin::Fragment>,
    /// The reader where the bindings leave their scope, or `None` where
    /// the part lends what it reads mutably for that scope alone and its
    /// bindings read copies instead.
    pub(super) escaping: Option<origin::Fragment>,
    /// Whether reading the part moves the fields out of the value.
    #[cfg_attr(
        not(feature = "rust-import"),
        expect(dead_code, reason = "only imported variants have parts")
    )]
    pub(super) moves: bool,
    /// Whether reading the part lends the fields mutably.
    #[cfg_attr(
        not(feature = "rust-import"),
        expect(dead_code, reason = "only imported variants have parts")
    )]
    pub(super) mutable: bool,
}

impl HostPart {
    /// The part of a variant's fields read by `reader`, which moves them
    /// out of the value when `guard` gives the reader that lends them
    /// instead, or lends them, mutably when `mutable`.
    #[cfg(feature = "rust-import")]
    pub(super) fn fields(
        name: String,
        reader: origin::Fragment,
        guard: Option<origin::Fragment>,
        mutable: bool,
    ) -> Self {
        let moves = guard.is_some();
        Self {
            name,
            escaping: (!mutable).then(|| reader.clone()),
            reader,
            guard,
            moves,
            mutable,
        }
    }

    /// Whether the part moves or mutably lends what it reads: what a
    /// pattern below the variant's reads then comes from the part, as
    /// nothing else may reach the value meanwhile.
    #[cfg(feature = "rust-import")]
    pub(super) const fn holds(&self) -> bool {
        self.moves || self.mutable
    }

    /// The reader of the part where it is read as `reading`.
    pub(super) fn reader(&self, reading: HostReading) -> Option<&origin::Fragment> {
        match reading {
            HostReading::Arm => Some(&self.reader),
            HostReading::Guard => Some(self.guard.as_ref().unwrap_or(&self.reader)),
            HostReading::Escaping => self.escaping.as_ref(),
        }
    }
}

impl Host {
    pub(super) fn bind(&mut self, binding: PatternBinding, value: origin::Fragment) {
        self.steps.push(HostStep::Bind(self.binds.len()));
        self.binds.push(binding);
        self.values.push(value);
    }

    /// Add `part`, bound before what is bound after it, and give its index.
    #[cfg(feature = "rust-import")]
    pub(super) fn part(&mut self, part: HostPart) -> usize {
        self.steps.push(HostStep::Part(self.parts.len()));
        self.parts.push(part);
        self.parts.len() - 1
    }

    /// Bind `binding` to member `index` of the part `part`, or to `copy`
    /// where the part is not read.
    #[cfg(feature = "rust-import")]
    pub(super) fn bind_part(
        &mut self,
        binding: PatternBinding,
        part: usize,
        index: usize,
        copy: origin::Fragment,
    ) {
        let mut value = origin::Fragment::plain(self.parts[part].name.as_str());
        value.push_text(format!("[{index}]"));
        self.readers.push((self.binds.len(), part, copy));
        self.bind(binding, value);
    }

    /// Whether a part moves what it reads, which the variant above it then
    /// moves out too.
    #[cfg(feature = "rust-import")]
    pub(super) fn moves(&self) -> bool {
        self.parts.iter().any(|part| part.moves)
    }

    /// Whether a part lends what it reads mutably, which the variant above
    /// it then lends mutably too.
    #[cfg(feature = "rust-import")]
    pub(super) fn lends_mutably(&self) -> bool {
        self.parts.iter().any(|part| part.mutable)
    }

    /// Take `other`'s bindings and the parts they read.
    pub(super) fn absorb_binds(&mut self, other: Self) {
        let (binds, parts) = (self.binds.len(), self.parts.len());
        self.readers.extend(
            other
                .readers
                .into_iter()
                .map(|(bind, part, copy)| (binds + bind, parts + part, copy)),
        );
        self.steps
            .extend(other.steps.into_iter().map(|step| match step {
                HostStep::Bind(index) => HostStep::Bind(binds + index),
                HostStep::Part(index) => HostStep::Part(parts + index),
            }));
        self.binds.extend(other.binds);
        self.values.extend(other.values);
        self.parts.extend(other.parts);
    }

    /// The tests joined by `&&`, or `None` when the pattern matches every
    /// value.
    pub(super) fn condition(&self) -> Option<origin::Fragment> {
        let mut tests = self.tests.iter();
        let mut joined = tests.next()?.clone().surrounded("(", ")");
        for test in tests {
            joined.push_text(" && ");
            joined.push(test.clone().surrounded("(", ")"));
        }
        Some(joined)
    }

    /// `let name = value; ..` for each binding, in the scope the bindings
    /// are used in.
    pub(super) fn rendered_binds(&self) -> origin::Fragment {
        self.rendered(HostReading::Arm)
    }

    /// The bindings as an arm's guard reads them (`HostReading::Guard`).
    pub(super) fn rendered_guard_binds(&self) -> origin::Fragment {
        self.rendered(HostReading::Guard)
    }

    /// The bindings as values that leave the scope they are made in
    /// (`HostReading::Escaping`).
    pub(super) fn rendered_escaping_binds(&self) -> origin::Fragment {
        self.rendered(HostReading::Escaping)
    }

    /// Each binding and part in order, a part by its reader where it is
    /// read as `reading`, and a binding that reads a part by copy where
    /// the part has none.
    pub(super) fn rendered(&self, reading: HostReading) -> origin::Fragment {
        let mut source = origin::Fragment::default();
        let mut bind = |name: &str, value: &origin::Fragment| {
            source.push_text(format!("let {name} = "));
            source.push(value.clone());
            source.push_text("; ");
        };
        for step in &self.steps {
            match *step {
                HostStep::Part(index) => {
                    let part = &self.parts[index];
                    if let Some(reader) = part.reader(reading) {
                        bind(&part.name, reader);
                    }
                }
                HostStep::Bind(index) => {
                    let value = self
                        .readers
                        .iter()
                        .find(|(reader, ..)| *reader == index)
                        .filter(|(_, part, _)| self.parts[*part].reader(reading).is_none())
                        .map_or(&self.values[index], |(.., copy)| copy);
                    bind(&self.binds[index].name, value);
                }
            }
        }
        source
    }
}

/// A resolved root match pattern: its runtime proof, its coverage, and the
/// bindings it declares.
pub(super) struct ResolvedRoot {
    pub(super) proof: NominalPattern,
    pub(super) coverage: coverage::Pattern,
    pub(super) span: Span,
    pub(super) bindings: Vec<PatternBinding>,
    /// What the pattern does to its subject's parts (`borrows.rs`).
    pub(super) trace: borrows::PatternTrace,
}

impl<'a> Parser<'a> {
    /// The bindings of `let pattern = subject`, after the statement, or of
    /// a `for` pattern, as `site` says. Without `otherwise` the pattern
    /// must be irrefutable, as rustc's E0005. A pattern matched on a `&mut`
    /// place matches it where it is, and its `&mut` bindings reach their
    /// parts of it (`PlacePattern`).
    #[allow(clippy::too_many_arguments)]
    pub(in crate::resolve) fn bind_let_pattern(
        &mut self,
        site: Site,
        start: Position,
        syntax: &Syntax,
        subject: ParsedExpr,
        matched: Option<Matched>,
        otherwise: Option<Branch>,
        end: Position,
        pending: usize,
    ) -> Result<(), Error> {
        let written_ty = self.inference.resolve(&subject.ty);
        let subject_ty = self.referenced(&written_ty);
        let mut resolved = self.resolve_root_pattern(syntax, &written_ty)?;
        let trace = std::mem::take(&mut resolved.trace);
        match matched {
            Some(matched) => {
                self.pattern_facts(trace, &matched, subject.span, &resolved.bindings)?;
            }
            None => self.pattern_unrecorded(&resolved.bindings, resolved.span),
        }
        if otherwise.is_none() {
            let site = match site {
                // The statement up to its `;`, where `end` is.
                Site::Let => BindingSite::Local {
                    statement: self
                        .written(span(start, end))
                        .map(|statement| statement.trim_end().into()),
                    binds: !resolved.bindings.is_empty(),
                },
                Site::For => BindingSite::ForLoop,
                Site::Argument => BindingSite::Argument,
            };
            self.require_irrefutable(syntax, &resolved.coverage, &written_ty, site)?;
        }
        let (scrutinee, values) = (self.temporary("match"), self.temporary("bound"));
        let place = self.pattern_place(&subject, &mut resolved.proof, &resolved.bindings)?;
        // A value made for the statement drops at its end, or before
        // `else`; a local's moved parts are left `()` in it.
        let made = self.drops()
            && subject.lvalue.is_none()
            && subject.source.as_str() != "self"
            && !self.subject_deref
            && self.needs_drop(&subject.ty, subject.span)?;
        let holder = if made {
            drops::Holder::Local(scrutinee.clone(), Vec::new())
        } else {
            self.subject_holder(&subject, &scrutinee, self.subject_deref)?
        };
        let bound = resolved
            .bindings
            .iter()
            .map(|binding| (binding.name.clone(), binding.span, binding.ty.clone()))
            .collect::<Vec<_>>();
        let (subject, operand) = match &place {
            Some(place) => (place.locate.clone(), place.operand(&scrutinee)),
            None => (
                self.pattern_subject(subject, &subject_ty)?,
                scrutinee.clone(),
            ),
        };
        let host = self.take_host_checks()?;
        let label = self.push_root_pattern(resolved.proof, &resolved.bindings, place.as_ref());
        // The bindings a facade pattern makes follow the script pattern's.
        let mut bindings = resolved.bindings;
        bindings.extend(host.binds.iter().cloned());
        let names = bindings
            .iter()
            .map(|binding| PlacePattern::runtime_name(place.as_ref(), &binding.name, binding.span))
            .collect::<Vec<_>>();
        let copies = self.shadow_copies(&drops::pattern_names(syntax))?;
        let mut rendered = origin::Fragment::plain(format!("let {scrutinee} = "));
        rendered.push(subject);
        rendered.push_text(format!(
            ";\nlet {values} = __rawr_switch {operand} {{\n{label}"
        ));
        if let Some(condition) = host.condition() {
            rendered.push_text(" if ");
            rendered.push(condition);
        }
        rendered.push_text(" => { ");
        rendered.push(host.rendered_escaping_binds());
        rendered.push_text(format!("[{}] }}", names.join(", ")));
        if let Some(otherwise) = otherwise {
            rendered.push_text(",\n_ => ");
            if made {
                rendered.push_text(format!(
                    "{{ {}",
                    drops::glue_call(&scrutinee, &self.drop_shape(&written_ty))
                ));
                rendered.push(otherwise.value.rendered_block());
                rendered.push_text(" }");
            } else {
                rendered.push(otherwise.value.rendered_block());
            }
        }
        rendered.push_text("\n};");
        let mut declared = String::new();
        for (index, (binding, name)) in bindings.iter().zip(&names).enumerate() {
            let keyword = self.binding_keyword(binding.mutable, &binding.ty);
            let _ = write!(declared, "\n{keyword} {name} = {values}[{index}];");
            self.bindings.insert(
                binding.name.clone(),
                Binding {
                    ty: binding.ty.clone(),
                    mutable: binding.mutable,
                    available: true,
                    declaration: binding.span,
                    kind: BindingKind::Local,
                },
            );
        }
        if let Some(place) = &place {
            self.bind_parts(place);
        }
        // The moves out of what was matched, then the rest of a value made
        // for it drops, before the bindings are declared.
        if self.drops() {
            let clears = self.pattern_drops(
                Some(syntax),
                &written_ty,
                &holder,
                &bound,
                drops::Clears::BeforeBindings,
            )?;
            rendered.push_text(format!("\n{clears}"));
            if made {
                rendered.push_text(drops::glue_call(&scrutinee, &self.drop_shape(&written_ty)));
            }
        }
        // Copies of the outer locals the bindings shadow, once the moves
        // left `()` in them.
        rendered.push_text(copies);
        rendered.push_text(declared);
        let rendered = self.with_field_entries(pending, rendered);
        self.write_output(rendered, span(start, end));
        self.last_type = Ty::Unit;
        self.last_expression = None;
        Ok(())
    }

    pub(super) fn parse_pattern_binding(&mut self, start: Position) -> Result<(), Error> {
        let pending = self.field_entries.len();
        let syntax = self.parse_pattern_no_alt()?;
        let annotation = if matches!(self.peek(), Token::Colon) {
            self.next();
            Some(self.parse_local_type()?)
        } else {
            None
        };
        self.expect(Token::Equals, "'=' after a `let` pattern")?;
        let borrowing =
            matches!(self.peek(), Token::Ampersand) && matches!(self.peek_second(), Token::Mut);
        let outer = self.mutable_borrow;
        if borrowing {
            self.mutable_borrow = Some(self.expression_stack.len() + 1);
        }
        // `let W(a) = iter.collect();`: the pattern's struct is the type
        // the value is expected to have, as its annotation would be.
        let expected = annotation.clone().or_else(|| match &syntax {
            Syntax::TupleStruct { ty, .. } | Syntax::UnitStruct { ty, .. } => Some(ty.clone()),
            _ => None,
        });
        let dereferenced = matches!(self.peek(), Token::Multiply);
        let value_start = self.flow().value_start();
        let value = self.parse_expr_expecting(expected.as_ref());
        self.mutable_borrow = outer;
        let mut value = value?;
        self.subject_deref = dereferenced;
        // The pattern reads, moves and borrows the parts it binds.
        let matched = self.matched(&value, value_start);
        // `let (a, b) = &mut pair;`: the bindings are the place's parts.
        if annotation.is_none()
            && !syntax.has_ref_mut()
            && value.lvalue.is_some()
            && match self.inference.resolve(&value.ty) {
                Ty::Reference { mutable: true, .. } => true,
                Ty::Option(inner) => matches!(*inner, Ty::Reference { mutable: true, .. }),
                _ => false,
            }
        {
            let end = self.position();
            self.terminate(start, Continuation::Let)?;
            return self.bind_let_pattern(
                Site::Let,
                start,
                &syntax,
                value,
                Some(matched),
                None,
                end,
                pending,
            );
        }
        self.reject_invocation(&value.ty, value.span, "kept in a `let` binding")?;
        if let Some(expected) = &annotation {
            self.coerce_expr(expected, &mut value)?;
        }
        // `let Wrapper(x) = w;` of a one-variant enum, `let Some(x) = o;`
        // or `let [a, ..] = slice;`: matched as `let .. else` is, after the
        // coverage check finds it irrefutable, or refused as rustc's E0005.
        if syntax.may_refute() || self.matches_unsized_slice(&syntax, &value.ty) {
            let end = self.position();
            self.terminate(start, Continuation::Let)?;
            return self.bind_let_pattern(
                Site::Let,
                start,
                &syntax,
                value,
                Some(matched),
                None,
                end,
                pending,
            );
        }
        let ty = annotation.unwrap_or_else(|| value.ty.clone());
        // `ref mut` bindings are the subject's places: it is not copied.
        let (whole, mut rendered) = if syntax.has_ref_mut() {
            let (root, place) = match &value.lvalue {
                Some(Lvalue::Binding { binding, .. }) => (binding.clone(), binding.to_string()),
                Some(Lvalue::Place { binding, place, .. }) => {
                    (binding.clone(), place.as_str().to_string())
                }
                _ => {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: "`ref mut` of a value that is not a local's place".into(),
                        source: value.span,
                    })
                }
            };
            self.place_root = Some(root);
            (place, origin::Fragment::default())
        } else if let Syntax::Discard { .. } = syntax
            && let Some(mut discarded) = self.discarded(&value)
        {
            // `let _ = v.iter().map(f);` runs no stage, as Rust's.
            discarded.push_text(";");
            (String::new(), discarded)
        } else {
            let whole = self.temporary("pattern");
            // A value holding a pointer or a channel's handle is no `const`,
            // whose copy would count (`binding_keyword`).
            let keyword = self.binding_keyword(false, &ty);
            let mut rendered = origin::Fragment::plain(format!("{keyword} {whole} = "));
            let temporaries = self.close_temporaries();
            let made = self.with_temporaries(&temporaries, value.rendered_source());
            rendered.push(made);
            rendered.push_text(";");
            (whole, rendered)
        };
        // Copies of the outer locals the pattern's bindings shadow, made
        // once the pattern's moves left `()` in them.
        let copies = if self.drops() {
            self.shadow_copies(&drops::pattern_names(&syntax))?
        } else {
            String::new()
        };
        let traced = self.begin_trace();
        let destructured = self.destructure(&syntax, &whole, &ty);
        let trace = self.end_trace(traced);
        self.pattern_facts(trace, &matched, value.span, &[])?;
        self.place_root = None;
        let destructured_at = rendered.len();
        rendered.push_text(destructured?);
        // With `impl Drop` in the crate, what the pattern moves out is left
        // `()` where it was. A value made for the statement drops what stays
        // in it when the statement ends, as in Rust.
        let mut made = false;
        if self.drops() && !whole.is_empty() && !syntax.has_ref_mut() {
            let (before, after, dropped) =
                self.let_pattern_drops(&syntax, &value, &ty, &whole, dereferenced)?;
            // The moves out of a local go before the bindings, which may
            // shadow it.
            let destructured = rendered.split_off(destructured_at);
            rendered.push_text(before);
            rendered.push_text(copies);
            rendered.push(destructured);
            rendered.push_text(after);
            made = dropped;
        }
        if !made && !whole.is_empty() && !syntax.has_ref_mut() && self.moves_by_value(&ty) {
            rendered.push_text(format!(
                " {}({}({whole}));",
                crate::engine::KEYWORD_DROP,
                crate::engine::KEYWORD_MOVE
            ));
        }
        let rendered = self.with_field_entries(pending, rendered);
        self.write_output(rendered, span(start, value.span.end()));
        self.last_type = Ty::Unit;
        self.last_expression = None;
        self.terminate(start, Continuation::Let).map(|_| ())
    }

    /// A pattern, with top-level alternatives: `p | q`.
    pub(super) fn parse_pattern_syntax(&mut self) -> Result<Syntax, Error> {
        if matches!(self.peek(), Token::Pipe) {
            // A leading `|`, as Rust allows.
            self.next();
        }
        let first = self.parse_pattern_no_alt()?;
        if !matches!(self.peek(), Token::Pipe) {
            return Ok(first);
        }
        let mut alternatives = vec![first];
        while matches!(self.peek(), Token::Pipe) {
            self.next();
            alternatives.push(self.parse_pattern_no_alt()?);
        }
        let span = Span::new(
            alternatives[0].span().start(),
            alternatives
                .last()
                .expect("or-patterns hold alternatives")
                .span()
                .end(),
        );
        Ok(Syntax::Or {
            alternatives: alternatives.into_boxed_slice(),
            span,
        })
    }

    /// One literal of a literal or range pattern, when the next tokens are one.
    pub(super) fn parse_pattern_literal(
        &mut self,
    ) -> Result<Option<(ast::NominalLiteral, Ty, Span)>, Error> {
        let negative = matches!(self.peek(), Token::Minus | Token::UnaryMinus);
        if !negative
            && !matches!(
                self.peek(),
                Token::IntegerConstant(_)
                    | Token::ByteConstant(_)
                    | Token::CharConstant(_)
                    | Token::StringConstant(_)
                    | Token::True
                    | Token::False
            )
        {
            if matches!(self.peek(), Token::FloatConstant(_)) {
                // Handled below.
            } else {
                return Ok(None);
            }
        }
        let start = if negative { Some(self.next().1) } else { None };
        let (token, position) = self.next();
        let start = start.unwrap_or(position);
        let (literal, ty, text) = match token {
            Token::IntegerConstant(value) => (
                ast::NominalLiteral::Int(if negative { -value } else { value }),
                self.literal_kind(value, true, point_span(position))?.0,
                value.to_string(),
            ),
            Token::FloatConstant(value) => (
                ast::NominalLiteral::Float(if negative {
                    crate::types::FloatWrapper::new(-*value.0)
                } else {
                    value.0
                }),
                self.literal_kind(0, false, point_span(position))?.0,
                value.1.to_string(),
            ),
            Token::ByteConstant(value) if !negative => (
                ast::NominalLiteral::Int(value.into()),
                Ty::Int(Int::U8),
                value.to_string(),
            ),
            Token::CharConstant(value) if !negative => (
                ast::NominalLiteral::Char(value),
                Ty::Char,
                format!("{value:?}"),
            ),
            Token::StringConstant(value) if !negative => (
                ast::NominalLiteral::Str(value.as_str().into()),
                Ty::Str,
                format!("{value:?}"),
            ),
            Token::True if !negative => (ast::NominalLiteral::Bool(true), Ty::Bool, "true".into()),
            Token::False if !negative => {
                (ast::NominalLiteral::Bool(false), Ty::Bool, "false".into())
            }
            _ => {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: "match pattern".into(),
                    source: point_span(position),
                })
            }
        };
        Ok(Some((
            literal,
            ty,
            Span::new(start, text_end(position, &text)),
        )))
    }

    /// A pattern without top-level alternatives, as closure parameters and
    /// `let` take.
    pub(super) fn parse_pattern_no_alt(&mut self) -> Result<Syntax, Error> {
        let start = self.position();
        // `todo!()` where a pattern goes: std's panicking macros expand to
        // a call, which no pattern is (E0532).
        if matches!(self.peek(), Token::Identifier(ref name)
                if matches!(name.as_str(), "todo" | "unimplemented" | "panic" | "unreachable"))
            && matches!(self.peek_second(), Token::Bang)
        {
            self.next();
            self.next();
            let unit = matches!(self.peek(), Token::Unit);
            let (_, end) = self.take_argument_tokens()?;
            let end = text_end(end, if unit { "()" } else { ")" });
            return Err(Error::StdPath {
                code: "E0532",
                message: "expected a pattern, found a function call".into(),
                label: Some("not a tuple struct or tuple variant".into()),
                note: Some(
                    "function calls are not allowed in patterns: \
                     <https://doc.rust-lang.org/book/ch19-00-patterns.html>"
                        .into(),
                ),
                reference: Span::new(start, end),
            });
        }
        if matches!(self.peek(), Token::Underscore) {
            self.next();
            let span = Span::new(start, text_end(start, "_"));
            return Ok(Syntax::Discard { span });
        }
        self.splice_std_alias();
        // `&p`, `&mut p`; `&&p` is two references.
        if matches!(self.peek(), Token::Ampersand | Token::And) {
            let double = matches!(self.next().0, Token::And);
            let mutable = matches!(self.peek(), Token::Mut);
            if mutable {
                self.next();
            }
            let pattern = self.parse_pattern_no_alt()?;
            let span = Span::new(start, pattern.span().end());
            let inner = Syntax::Reference {
                mutable,
                pattern: Box::new(pattern),
                span,
            };
            return Ok(if double {
                Syntax::Reference {
                    mutable: false,
                    pattern: Box::new(inner),
                    span,
                }
            } else {
                inner
            });
        }
        // `..`, an element of a slice or tuple pattern.
        if matches!(self.peek(), Token::ExclusiveRange) {
            self.next();
            return Ok(Syntax::Rest {
                binding: None,
                span: Span::new(start, text_end(start, "..")),
            });
        }
        if matches!(self.peek(), Token::LeftBracket) {
            return self.parse_slice_pattern(start);
        }
        if matches!(self.peek(), Token::LeftParen | Token::Unit) {
            // `()` matches the one unit value, as `Ok(())` does.
            if matches!(self.next().0, Token::Unit) {
                let span = Span::new(start, text_end(start, "()"));
                return Ok(Syntax::Discard { span });
            }
            let mut members = Vec::new();
            let mut trailing_comma = false;
            while !matches!(self.peek(), Token::RightParen | Token::EOF) {
                members.push(self.parse_pattern_syntax()?);
                trailing_comma = matches!(self.peek(), Token::Comma);
                if trailing_comma {
                    self.next();
                } else if !matches!(self.peek(), Token::RightParen) {
                    return Err(self.unsupported("tuple pattern members without ','"));
                }
            }
            let end = self.expect(Token::RightParen, "')' to close a tuple pattern")?;
            // `(p)` is `p`, as in Rust; `(p,)` is a one-tuple.
            if members.len() == 1 && !trailing_comma {
                return Ok(members.pop().expect("one member"));
            }
            return Ok(Syntax::Tuple {
                members: members.into_boxed_slice(),
                span: span(start, end),
            });
        }
        // `b"tag1"` on a `&[u8]`: the slice pattern of its bytes, as rustc
        // matches it.
        if let Token::ByteStringConstant(bytes) = self.peek() {
            let bytes = bytes.clone();
            let (token, at) = self.next();
            let span = Span::new(at, text_end(at, &token.to_string()));
            let before = bytes
                .iter()
                .map(|byte| Syntax::Literal {
                    literal: ast::NominalLiteral::Int(i64::from(*byte)),
                    ty: Ty::Int(Int::U8),
                    span,
                })
                .collect();
            return Ok(Syntax::Slice {
                before,
                rest: None,
                after: Box::default(),
                span,
            });
        }
        if let Some((low, low_ty, low_span)) = self.parse_pattern_literal()? {
            let inclusive = match self.peek() {
                Token::InclusiveRange => true,
                Token::ExclusiveRange => false,
                _ => {
                    return Ok(Syntax::Literal {
                        literal: low,
                        ty: low_ty,
                        span: low_span,
                    })
                }
            };
            let operator = self.next().1;
            let high = self.parse_pattern_literal()?;
            if inclusive && high.is_none() {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: "an inclusive range pattern without an upper bound".into(),
                    source: point_span(operator),
                });
            }
            let end = high
                .as_ref()
                .map_or(text_end(operator, ".."), |(_, _, span)| span.end());
            return Ok(Syntax::Range {
                low: Some((low, low_ty)),
                high: high.map(|(literal, ty, _)| (literal, ty)),
                inclusive,
                span: Span::new(low_span.start(), end),
            });
        }
        if matches!(self.peek(), Token::InclusiveRange) {
            // `..=high`.
            let operator = self.next().1;
            let Some((high, ty, high_span)) = self.parse_pattern_literal()? else {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: "a range pattern without an upper bound".into(),
                    source: point_span(operator),
                });
            };
            return Ok(Syntax::Range {
                low: None,
                high: Some((high, ty)),
                inclusive: true,
                span: Span::new(start, high_span.end()),
            });
        }
        if let Some(pattern) = self.parse_ref_binding()? {
            return Ok(pattern);
        }
        let mutable = matches!(self.peek(), Token::Mut);
        if mutable {
            self.next();
        }

        let head = if matches!(self.peek(), Token::SelfValue)
            && matches!(self.peek_second(), Token::DoubleColon)
        {
            ("self".into(), point_span(self.next().1))
        } else {
            self.expect_name("payload binding or enum name in match pattern")?
        };
        let mut segments = vec![head];
        while matches!(self.peek(), Token::DoubleColon) {
            self.next();
            segments.push(self.expect_qualified_name()?);
        }
        if let [(head, head_span), _, ..] = segments.as_slice() {
            self.unbound_std_type(head, true, *head_span)?;
        }
        // A path into a `.rs` module, `echo::Event::Connected`, is spelled
        // through the app crate, as the same path is in an expression.
        if segments.len() >= 2 {
            let first_span = segments[0].1;
            let (last, last_span) = &segments[segments.len() - 1];
            let last_end = text_end(last_span.start(), last);
            if let Some(spelled) = self
                .models
                .qualified_rust(Span::new(first_span.start(), last_end))
            {
                let variant = segments
                    .pop()
                    .expect("a qualified pattern keeps its variant");
                segments = spelled[..spelled.len() - 1]
                    .iter()
                    .map(|name| (name.clone(), first_span))
                    .collect();
                segments.push(variant);
            }
        }
        if mutable && segments.len() > 1 {
            return Err(self.unsupported("`mut` before a path pattern"));
        }
        // `name @ pattern`.
        if segments.len() == 1
            && matches!(self.peek(), Token::Reserved(ref text) if text.as_str() == "@")
        {
            self.next();
            let (name, name_span) = segments.pop().expect("one segment");
            if mutable {
                return Err(self.unsupported("`mut` with an `@` binding"));
            }
            // `name @ ..`, the rest of a slice pattern.
            if matches!(self.peek(), Token::ExclusiveRange) {
                let (_, rest) = self.next();
                return Ok(Syntax::Rest {
                    binding: Some((name, name_span)),
                    span: Span::new(name_span.start(), text_end(rest, "..")),
                });
            }
            let pattern = self.parse_pattern_no_alt()?;
            return Ok(Syntax::At {
                name,
                name_span,
                pattern: Box::new(pattern),
            });
        }
        // `Ordering::Less`, `std::cmp::Ordering::Less`: its discriminant.
        if let [path @ .., (variant, variant_span)] = segments.as_slice()
            && !mutable
            && let Some(value) = ordering_discriminant(variant)
            && self.ordering_path(path.iter().map(|(name, _)| name.as_str()))
        {
            return Ok(Syntax::Literal {
                literal: ast::NominalLiteral::Int(value),
                ty: Ty::Ordering,
                span: Span::new(
                    segments[0].1.start(),
                    text_end(variant_span.start(), variant),
                ),
            });
        }
        // `Cow::Borrowed(..)` and `Cow::Owned(..)`.
        let written = segments
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>();
        if !mutable && let Some(owned) = self.cow_variant(&written) {
            let start = segments[0].1.start();
            self.expect(Token::LeftParen, "'(' after a `Cow` variant in a pattern")?;
            let payload = self.parse_pattern_syntax()?;
            if matches!(self.peek(), Token::Comma) {
                self.next();
            }
            let end = self.expect(Token::RightParen, "')' after a `Cow` variant pattern")?;
            return Ok(Syntax::Cow {
                owned,
                payload: Box::new(payload),
                span: span(start, end),
            });
        }
        // The prelude's `Some(..)`, `None`, `Ok(..)` and `Err(..)`, also as
        // `Option::Some(..)` or `Result::Ok(..)`.
        let prelude = match segments.as_slice() {
            [(name, span)] if !mutable => Prelude::from_name(name).map(|variant| (variant, *span)),
            [(owner, _), (name, span)] => Prelude::from_name(name)
                .filter(|variant| {
                    owner.as_str() == variant.owner() && self.models.get(variant.owner()).is_none()
                })
                .map(|variant| (variant, *span)),
            _ => None,
        };
        if let Some((variant, name_span)) = prelude {
            let start = segments[0].1.start();
            if variant == Prelude::None {
                let end = text_end(name_span.start(), variant.name());
                return Ok(Syntax::Prelude {
                    variant,
                    payload: None,
                    span: Span::new(start, end),
                });
            }
            self.expect(Token::LeftParen, "'(' after a prelude variant in a pattern")?;
            let payload = self.parse_pattern_syntax()?;
            if matches!(self.peek(), Token::Comma) {
                self.next();
            }
            let end = self.expect(Token::RightParen, "')' after a prelude variant pattern")?;
            return Ok(Syntax::Prelude {
                variant,
                payload: Some(Box::new(payload)),
                span: span(start, end),
            });
        }
        // `W(a, b)`, `module::W(a)`, `module::S`.
        if !mutable {
            let (last, last_span) = segments.last().expect("a pattern path");
            let path = Span::new(segments[0].1.start(), text_end(last_span.start(), last));
            let model = if segments.len() == 1 {
                self.named_model(last)
            } else {
                self.models.qualified_owner(path).cloned()
            };
            if let Some(model @ Nominal::Struct { .. }) = model {
                let Nominal::Struct { item, .. } = &model else {
                    unreachable!("matched a struct");
                };
                if item.form == crate::item::StructForm::Tuple
                    && matches!(self.peek(), Token::LeftParen | Token::Unit)
                {
                    return self.parse_tuple_struct_pattern(model, path);
                }
                if item.form != crate::item::StructForm::Tuple
                    && matches!(self.peek(), Token::LeftBrace)
                {
                    return self.parse_struct_pattern(model, path);
                }
                if segments.len() > 1
                    && item.form == crate::item::StructForm::Unit
                    && item.generics.is_empty()
                {
                    return Ok(Syntax::UnitStruct {
                        ty: model.ty(),
                        span: path,
                    });
                }
            }
        }
        // `Point { x, y }` on a facade struct, which a script struct's
        // pattern above did not take.
        if !mutable
            && matches!(self.peek(), Token::LeftBrace)
            && segments
                .last()
                .is_some_and(|(name, _)| name.starts_with(char::is_uppercase))
            && matches!(
                self.models.host.entry(&self.host_pattern_owner(&segments)),
                Some(crate::host::Entry::Type(_))
            )
        {
            let (variant, variant_span) = segments.last().cloned().expect("a pattern path");
            let owner_span = Span::new(
                segments[0].1.start(),
                text_end(variant_span.start(), &variant),
            );
            let (fields, rest, _) = self.parse_field_patterns()?;
            let mut names = Vec::with_capacity(fields.len());
            let mut payload = Vec::with_capacity(fields.len());
            for (name, name_span, pattern) in fields {
                names.push((name, name_span));
                payload.push(pattern);
            }
            return Ok(Syntax::Enum(EnumSyntax {
                owner: segments.into_boxed_slice(),
                owner_span,
                variant,
                variant_span,
                payload: payload.into_boxed_slice(),
                named: Some((names.into_boxed_slice(), rest)),
                host_struct: true,
            }));
        }
        // `badge::Secondary` and an imported `Secondary`: a crate's unit
        // struct, as rustc reads the path in the value namespace.
        #[cfg(feature = "rust-import")]
        if !mutable && let Some(ty) = self.imported_unit_struct(&segments) {
            let (last, last_span) = segments.last().expect("a pattern path");
            return Ok(Syntax::UnitStruct {
                ty,
                span: Span::new(segments[0].1.start(), text_end(last_span.start(), last)),
            });
        }
        if segments.len() == 1 {
            let (name, span) = segments
                .pop()
                .expect("one-segment patterns retain their binding");
            if !mutable && let Some(ty) = self.unit_struct(&name) {
                return Ok(Syntax::UnitStruct { ty, span });
            }
            return Ok(Syntax::Binding {
                name,
                span,
                mutable,
            });
        }

        let (variant, variant_span) = segments
            .pop()
            .expect("qualified patterns retain their variant");
        let owner_end = segments
            .last()
            .map(|(name, span)| text_end(span.start(), name))
            .expect("qualified patterns retain an owner");
        let owner_span = Span::new(segments[0].1.start(), owner_end);
        let mut payload = Vec::new();
        let mut named = None;
        if matches!(self.peek(), Token::LeftBrace) {
            let (fields, rest, _) = self.parse_field_patterns()?;
            let mut names = Vec::with_capacity(fields.len());
            for (name, name_span, pattern) in fields {
                names.push((name, name_span));
                payload.push(pattern);
            }
            named = Some((names.into_boxed_slice(), rest));
        } else if matches!(self.peek(), Token::Unit) {
            self.next();
        } else if matches!(self.peek(), Token::LeftParen) {
            self.next();
            while !matches!(self.peek(), Token::RightParen | Token::EOF) {
                payload.push(self.parse_pattern_syntax()?);
                if matches!(self.peek(), Token::Comma) {
                    self.next();
                } else if !matches!(self.peek(), Token::RightParen) {
                    return Err(self.unsupported("payload patterns in a match pattern require ','"));
                }
            }
            self.expect(Token::RightParen, "')' after match payload patterns")?;
        }

        Ok(Syntax::Enum(EnumSyntax {
            owner: segments.into_boxed_slice(),
            owner_span,
            variant,
            variant_span,
            payload: payload.into_boxed_slice(),
            named,
            host_struct: false,
        }))
    }

    /// `[a, b, rest @ .., z]`, after its `[` at `start`.
    pub(super) fn parse_slice_pattern(&mut self, start: Position) -> Result<Syntax, Error> {
        self.next();
        let (mut before, mut rest, mut after) = (Vec::new(), None, Vec::new());
        while !matches!(self.peek(), Token::RightBracket | Token::EOF) {
            let element = self.parse_pattern_syntax()?;
            if let Syntax::Rest { span, .. } = &element {
                if rest.is_some() {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: "`..` can only be used once per slice pattern".into(),
                        source: *span,
                    });
                }
                rest = Some(Box::new(element));
            } else if rest.is_some() {
                after.push(element);
            } else {
                before.push(element);
            }
            if matches!(self.peek(), Token::Comma) {
                self.next();
            } else if !matches!(self.peek(), Token::RightBracket) {
                return Err(self.unsupported("slice pattern elements without ','"));
            }
        }
        let end = self.expect(Token::RightBracket, "']' to close a slice pattern")?;
        Ok(Syntax::Slice {
            before: before.into_boxed_slice(),
            rest,
            after: after.into_boxed_slice(),
            span: span(start, end),
        })
    }

    /// One pattern nested in a payload of type `expected`.
    pub(super) fn resolve_child_pattern(
        &self,
        syntax: &Syntax,
        expected: &Ty,
        owner: &str,
        variant: &str,
        bound_names: &mut BTreeMap<ImmutableString, Span>,
        binding_declarations: &mut Vec<PatternBinding>,
    ) -> Result<(NominalPattern, coverage::Pattern), Error> {
        let expected = self.inference.resolve(expected);
        let path = self.trace_length();
        // Matching a constructor, literal or tuple through `&T` matches the
        // referenced value, and the bindings below it are references: Rust's
        // default binding mode.
        let (expected, outer_mode) = match (&expected, syntax) {
            (Ty::Reference { target, mutable }, syntax)
                if !syntax.is_irrefutable_leaf()
                    && !matches!(
                        syntax,
                        Syntax::Reference { .. } | Syntax::At { .. } | Syntax::Or { .. }
                    ) =>
            {
                // `&&T` matches the `T` below both, as rustc does.
                let mut mutable = *mutable;
                let mut target = self.inference.resolve(target);
                self.trace_enter(borrowck::Projection::Deref);
                while let Ty::Reference {
                    target: inner,
                    mutable: inner_mutable,
                } = target
                {
                    mutable = mutable && inner_mutable;
                    target = self.inference.resolve(&inner);
                    self.trace_enter(borrowck::Projection::Deref);
                }
                let mode = self.binding_mode.get();
                self.binding_mode
                    .set(Some(mode.map_or(mutable, |outer| outer && mutable)));
                (target, Some(mode))
            }
            _ => (expected, None),
        };
        let resolved = self.resolve_child_pattern_in_mode(
            syntax,
            &expected,
            owner,
            variant,
            bound_names,
            binding_declarations,
        );
        if let Some(mode) = outer_mode {
            self.binding_mode.set(mode);
        }
        self.trace_leave(path);
        resolved
    }

    pub(super) fn resolve_child_pattern_in_mode(
        &self,
        syntax: &Syntax,
        expected: &Ty,
        owner: &str,
        variant: &str,
        bound_names: &mut BTreeMap<ImmutableString, Span>,
        binding_declarations: &mut Vec<PatternBinding>,
    ) -> Result<(NominalPattern, coverage::Pattern), Error> {
        let expected = expected.clone();
        // A refutable pattern on a facade value: bind the value to a hidden
        // name, and leave its tests to the pattern's site.
        if matches!(expected, Ty::Host { .. })
            && !matches!(
                syntax,
                Syntax::Binding { .. }
                    | Syntax::Ref { .. }
                    | Syntax::Discard { .. }
                    | Syntax::At { .. }
                    | Syntax::Reference { .. }
            )
        {
            // The facade's tests read the part; what they bind is not in
            // the borrow facts.
            self.trace_test(borrowck::Access::Read, None);
            if !drops::pattern_names(syntax).is_empty() {
                self.trace_unmodeled("a pattern binding a part of a host value", syntax.span());
            }
            let mut checks = self.host_checks.borrow_mut();
            let name: ImmutableString = format!("__rawr_host_{}", checks.len()).into();
            checks.push((
                name.clone(),
                syntax.clone(),
                expected.clone(),
                self.binding_mode.get(),
            ));
            drop(checks);
            binding_declarations.push(PatternBinding {
                name: name.clone(),
                span: syntax.span(),
                ty: expected.clone(),
                mutable: false,
            });
            return Ok((
                NominalPattern::Binding {
                    name,
                    expected: self.payload_type(&expected),
                },
                coverage::Pattern::Wildcard,
            ));
        }
        match syntax {
            // `ref mut x` binds `&mut` to the part, which the pattern's
            // place takes back after each statement that uses it.
            Syntax::Ref {
                name,
                span,
                mutable,
            } => {
                // `ref y` borrows the part whatever its type: on an `&i64`
                // member it binds `&&i64`.
                let expected = Ty::Reference {
                    mutable: *mutable,
                    target: Box::new(expected),
                };
                let binding = Syntax::Binding {
                    name: name.clone(),
                    span: *span,
                    mutable: false,
                };
                self.trace_ref(*mutable);
                let mode = self.binding_mode.replace(None);
                let resolved = self.resolve_child_pattern_in_mode(
                    &binding,
                    &expected,
                    owner,
                    variant,
                    bound_names,
                    binding_declarations,
                );
                self.binding_mode.set(mode);
                resolved
            }
            Syntax::Binding {
                name,
                span,
                mutable,
            } => {
                if bound_names.insert(name.clone(), *span).is_some() {
                    return Err(Error::InvalidMatch {
                        owner: owner.into(),
                        reason: format!("binding `{name}` appears more than once in `{variant}`")
                            .into(),
                        pattern: *span,
                    });
                }
                let ty = match self.binding_mode.get() {
                    None => expected.clone(),
                    Some(mutable) => Ty::Reference {
                        mutable,
                        target: Box::new(expected.clone()),
                    },
                };
                self.trace_bind(name, *span, &ty);
                binding_declarations.push(PatternBinding {
                    name: name.clone(),
                    span: *span,
                    ty,
                    mutable: *mutable,
                });
                Ok((
                    NominalPattern::Binding {
                        name: name.clone(),
                        expected: self.payload_type(&expected),
                    },
                    coverage::Pattern::Wildcard,
                ))
            }
            Syntax::Discard { .. } => Ok((
                NominalPattern::Discard {
                    expected: self.payload_type(&expected),
                },
                coverage::Pattern::Wildcard,
            )),
            Syntax::UnitStruct { ty, span } => {
                self.require_pattern_type(&expected, ty, *span)?;
                Ok((
                    NominalPattern::Discard {
                        expected: self.payload_type(&expected),
                    },
                    coverage::Pattern::Wildcard,
                ))
            }
            Syntax::Enum(pattern) => {
                let resolved = self.resolve_enum_pattern(
                    pattern,
                    &expected,
                    bound_names,
                    binding_declarations,
                )?;
                Ok((NominalPattern::Enum(resolved.proof), resolved.coverage))
            }
            Syntax::Prelude { .. } => {
                self.resolve_prelude_pattern(syntax, &expected, bound_names, binding_declarations)
            }
            Syntax::Cow {
                owned,
                payload,
                span,
            } => self.resolve_cow_pattern(
                *owned,
                payload,
                *span,
                &expected,
                bound_names,
                binding_declarations,
            ),
            Syntax::Literal { literal, ty, span } => {
                self.require_pattern_type(&expected, ty, *span)?;
                self.trace_test(borrowck::Access::Read, None);
                Ok((
                    NominalPattern::Literal(literal.clone()),
                    coverage::Pattern::Constructor {
                        key: coverage::Key::Literal(literal.clone()),
                        name: render_literal(literal).into(),
                        payload: Box::default(),
                    },
                ))
            }
            Syntax::Range {
                low,
                high,
                inclusive,
                span,
            } => {
                for (_, ty) in low.iter().chain(high.iter()) {
                    if !matches!(ty, Ty::Int(_) | Ty::Char | Ty::Float(_)) {
                        return Err(Error::InvalidMatch {
                            owner: owner.into(),
                            reason: "range patterns take integer, `char` or float bounds".into(),
                            pattern: *span,
                        });
                    }
                    self.require_pattern_type(&expected, ty, *span)?;
                }
                reject_empty_range(low.as_ref(), high.as_ref(), *inclusive, owner, *span)?;
                self.trace_test(borrowck::Access::Read, None);
                Ok((
                    NominalPattern::Range {
                        low: low.as_ref().map(|(literal, _)| literal.clone()),
                        high: high.as_ref().map(|(literal, _)| literal.clone()),
                        inclusive: *inclusive,
                    },
                    coverage::Pattern::Constructor {
                        key: coverage::Key::Range,
                        name: "..".into(),
                        payload: Box::default(),
                    },
                ))
            }
            Syntax::TupleStruct {
                definition,
                members,
                span,
                ..
            } => {
                // The fields under the scrutinee's instance, as an enum
                // pattern's payload.
                let (Ty::Script(id, arguments), Nominal::Struct { item, .. }) =
                    (&expected, self.models.definition(definition.id()))
                else {
                    return Err(Error::TypeMismatch {
                        expected: expected.name(&self.models),
                        actual: definition.name().into(),
                        expression: *span,
                    });
                };
                if *id != definition.id() {
                    return Err(Error::TypeMismatch {
                        expected: expected.name(&self.models),
                        actual: definition.name().into(),
                        expression: *span,
                    });
                }
                let substitution = self.nominal_substitution(*id, arguments);
                let fields = item
                    .fields
                    .iter()
                    .map(|field| instantiate(&Ty::from_resolved(&field.ty), &substitution))
                    .collect::<Vec<_>>();
                let mut proofs = Vec::with_capacity(members.len());
                let mut coverage = Vec::with_capacity(members.len());
                for (slot, ((member, ty), field)) in members
                    .iter()
                    .zip(fields.iter())
                    .zip(&item.fields)
                    .enumerate()
                {
                    let path = self.trace_enter_at(
                        borrowck::Projection::Field(field.name.text.clone()),
                        borrows::TraceRegion::Field(*id, None, slot),
                    );
                    let (proof, member_coverage) = self.resolve_child_pattern(
                        member,
                        ty,
                        owner,
                        variant,
                        bound_names,
                        binding_declarations,
                    )?;
                    self.trace_leave(path);
                    proofs.push(proof);
                    coverage.push(member_coverage);
                }
                Ok((
                    NominalPattern::Struct(proofs.into_boxed_slice()),
                    coverage::Pattern::Constructor {
                        key: coverage::Key::Tuple,
                        name: ImmutableString::new(),
                        payload: coverage.into_boxed_slice(),
                    },
                ))
            }
            Syntax::Tuple { members, span } => {
                let Ty::Tuple(types) = &expected else {
                    return Err(Error::TypeMismatch {
                        expected: expected.name(&self.models),
                        actual: format!("a tuple of {} elements", members.len()).into(),
                        expression: *span,
                    });
                };
                let discard = Syntax::Discard { span: *span };
                let members = tuple_members(members, types.len(), &discard)?;
                if types.len() != members.len() {
                    return Err(Error::TypeMismatch {
                        expected: expected.name(&self.models),
                        actual: format!("a tuple of {} elements", members.len()).into(),
                        expression: *span,
                    });
                }
                let mut proofs = Vec::with_capacity(members.len());
                let mut coverage = Vec::with_capacity(members.len());
                for (index, (member, ty)) in members.iter().zip(types.iter()).enumerate() {
                    let path = self.trace_enter_at(
                        borrowck::Projection::Field(index.to_string().into()),
                        borrows::TraceRegion::At(borrowck::Path::of(&[borrowck::Step::Arg(
                            index as u8,
                        )])),
                    );
                    let (proof, member_coverage) = self.resolve_child_pattern(
                        member,
                        ty,
                        owner,
                        variant,
                        bound_names,
                        binding_declarations,
                    )?;
                    self.trace_leave(path);
                    proofs.push(proof);
                    coverage.push(member_coverage);
                }
                Ok((
                    NominalPattern::Tuple(proofs.into_boxed_slice()),
                    coverage::Pattern::Constructor {
                        key: coverage::Key::Tuple,
                        name: ImmutableString::new(),
                        payload: coverage.into_boxed_slice(),
                    },
                ))
            }
            Syntax::Reference {
                mutable,
                pattern,
                span,
            } => {
                let Ty::Reference {
                    mutable: actual,
                    target,
                } = &expected
                else {
                    return Err(Error::TypeMismatch {
                        expected: expected.name(&self.models),
                        actual: if *mutable { "&mut _" } else { "&_" }.into(),
                        expression: *span,
                    });
                };
                if *mutable && !actual {
                    return Err(Error::TypeMismatch {
                        expected: expected.name(&self.models),
                        actual: "&mut _".into(),
                        expression: *span,
                    });
                }
                // References are the referenced value at runtime; an explicit
                // `&` pattern binds by value below it.
                let mode = self.binding_mode.replace(None);
                let path = self.trace_enter(borrowck::Projection::Deref);
                let resolved = self.resolve_child_pattern(
                    pattern,
                    target,
                    owner,
                    variant,
                    bound_names,
                    binding_declarations,
                );
                self.trace_leave(path);
                self.binding_mode.set(mode);
                resolved
            }
            Syntax::At {
                name,
                name_span,
                pattern,
            } => {
                if bound_names.insert(name.clone(), *name_span).is_some() {
                    return Err(Error::InvalidMatch {
                        owner: owner.into(),
                        reason: format!("binding `{name}` appears more than once in `{variant}`")
                            .into(),
                        pattern: *name_span,
                    });
                }
                // Below a reference the binding is one, as a plain binding.
                let ty = match self.binding_mode.get() {
                    None => expected.clone(),
                    Some(mutable) => Ty::Reference {
                        mutable,
                        target: Box::new(expected.clone()),
                    },
                };
                self.trace_bind(name, *name_span, &ty);
                binding_declarations.push(PatternBinding {
                    name: name.clone(),
                    span: *name_span,
                    ty,
                    mutable: false,
                });
                let (proof, coverage) = self.resolve_child_pattern(
                    pattern,
                    &expected,
                    owner,
                    variant,
                    bound_names,
                    binding_declarations,
                )?;
                Ok((
                    NominalPattern::At {
                        name: name.clone(),
                        pattern: Box::new(proof),
                        part: false,
                    },
                    coverage,
                ))
            }
            Syntax::Slice {
                before,
                rest,
                after,
                span,
            } => {
                let Ty::Array { element, kind } = &expected else {
                    return Err(Error::InvalidMatch {
                        owner: owner.into(),
                        reason: format!(
                            "expected an array or slice, found `{}`",
                            expected.name(&self.models)
                        )
                        .into(),
                        pattern: *span,
                    });
                };
                // A `Vec` or a `VecDeque` is no slice (rustc's E0529).
                if matches!(kind, sequence::Kind::Vec | sequence::Kind::Deque) {
                    return Err(Error::InvalidMatch {
                        owner: owner.into(),
                        reason: format!(
                            "expected an array or slice, found `{}` (E0529; match `v.as_slice()` \
                             or `&v[..]`)",
                            expected.name(&self.models)
                        )
                        .into(),
                        pattern: *span,
                    });
                }
                let length = kind.length();
                let fixed = before.len() + after.len();
                self.slice_arity.set(self.slice_arity.get().max(fixed));
                // Rust's E0527 and E0528.
                match length {
                    Some(length) if rest.is_none() && fixed != length => {
                        return Err(Error::InvalidMatch {
                            owner: owner.into(),
                            reason: format!(
                                "pattern requires {fixed} elements but array has {length}"
                            )
                            .into(),
                            pattern: *span,
                        })
                    }
                    Some(length) if fixed > length => {
                        return Err(Error::InvalidMatch {
                            owner: owner.into(),
                            reason: format!(
                                "pattern requires at least {fixed} elements but array has {length}"
                            )
                            .into(),
                            pattern: *span,
                        })
                    }
                    _ => {}
                }
                // A slice's length is tested; an array's is known.
                if length.is_none() {
                    self.trace_test(borrowck::Access::Length, Some(*span));
                }
                let mut resolve = |members: &[Syntax], after: bool| {
                    let mut proofs = Vec::with_capacity(members.len());
                    let mut coverage = Vec::with_capacity(members.len());
                    for (index, member) in members.iter().enumerate() {
                        let step =
                            borrows::slice_element(index, members.len(), after, fixed, length);
                        let path = self.trace_enter(step);
                        let (proof, member_coverage) = self.resolve_child_pattern(
                            member,
                            element,
                            owner,
                            variant,
                            bound_names,
                            binding_declarations,
                        )?;
                        self.trace_leave(path);
                        proofs.push(proof);
                        coverage.push(member_coverage);
                    }
                    Ok::<_, Error>((proofs, coverage))
                };
                let (before_proofs, mut coverage) = resolve(before, false)?;
                let (after_proofs, after_coverage) = resolve(after, true)?;
                coverage.extend(after_coverage);
                // `name @ ..` binds the middle: an array of the remaining
                // length, or a slice.
                let rest_proof = match rest.as_deref() {
                    Some(Syntax::Rest {
                        binding: Some((name, name_span)),
                        ..
                    }) => {
                        let middle = Ty::Array {
                            element: element.clone(),
                            kind: match length {
                                Some(length) => sequence::Kind::Array(length - fixed),
                                None => sequence::Kind::Slice,
                            },
                        };
                        let binding = Syntax::Binding {
                            name: name.clone(),
                            span: *name_span,
                            mutable: false,
                        };
                        // A subslice is not a place these facts name.
                        self.trace_unmodeled("a slice pattern binding its middle", *name_span);
                        let traced = self.begin_trace();
                        let resolved = self.resolve_child_pattern_in_mode(
                            &binding,
                            &middle,
                            owner,
                            variant,
                            bound_names,
                            binding_declarations,
                        );
                        self.end_trace(traced);
                        Some(Box::new(resolved?.0))
                    }
                    Some(_) => Some(Box::new(NominalPattern::Discard {
                        expected: NominalPayloadType::Any,
                    })),
                    None => None,
                };
                Ok((
                    NominalPattern::Slice {
                        before: before_proofs.into_boxed_slice(),
                        rest: rest_proof,
                        after: after_proofs.into_boxed_slice(),
                    },
                    coverage::Pattern::Constructor {
                        key: coverage::Key::Slice {
                            before: before.len(),
                            rest: rest.is_some(),
                        },
                        name: ImmutableString::new(),
                        payload: coverage.into_boxed_slice(),
                    },
                ))
            }
            Syntax::Rest { span, .. } => Err(Error::InvalidMatch {
                owner: owner.into(),
                reason: "`..` patterns are not allowed here".into(),
                pattern: *span,
            }),
            Syntax::Or { alternatives, .. } => {
                let mut proofs = Vec::with_capacity(alternatives.len());
                let mut coverage = Vec::with_capacity(alternatives.len());
                let mut first: Option<BTreeMap<ImmutableString, Ty>> = None;
                let outer_names = bound_names.clone();
                for alternative in alternatives.iter() {
                    let mut names = outer_names.clone();
                    let mut declarations = Vec::new();
                    let (proof, alternative_coverage) = self.resolve_child_pattern(
                        alternative,
                        &expected,
                        owner,
                        variant,
                        &mut names,
                        &mut declarations,
                    )?;
                    let bound = declarations
                        .iter()
                        .map(|binding| (binding.name.clone(), binding.ty.clone()))
                        .collect::<BTreeMap<_, _>>();
                    match &first {
                        None => {
                            *bound_names = names;
                            binding_declarations.extend(declarations);
                            first = Some(bound);
                        }
                        Some(first) => {
                            // A name has one type in every alternative. The
                            // root pattern makes them one, as rustc does with
                            // `(Some(x), _) | (None, Some(x))` on literals'
                            // `{integer}`s and `None`'s unknown type.
                            let same = first.len() == bound.len()
                                && first.keys().all(|name| bound.contains_key(name));
                            if same {
                                self.or_binding_types.borrow_mut().extend(first.iter().map(
                                    |(name, ty)| {
                                        (
                                            ty.clone(),
                                            bound[name].clone(),
                                            owner.to_string(),
                                            alternative.span(),
                                        )
                                    },
                                ));
                            } else {
                                return Err(Error::InvalidMatch {
                                    owner: owner.into(),
                                    reason: "every alternative of an or-pattern must bind the \
                                             same names with the same types"
                                        .into(),
                                    pattern: alternative.span(),
                                });
                            }
                        }
                    }
                    proofs.push(proof);
                    coverage.push(alternative_coverage);
                }
                Ok((
                    NominalPattern::Or(proofs.into_boxed_slice()),
                    coverage::Pattern::Or(coverage.into_boxed_slice()),
                ))
            }
        }
    }

    /// A literal or range bound of type `actual` where `expected` is matched.
    pub(super) fn require_pattern_type(
        &self,
        expected: &Ty,
        actual: &Ty,
        span: Span,
    ) -> Result<(), Error> {
        let expected = match self.inference.resolve(expected) {
            Ty::Reference { target, .. } => self.inference.resolve(&target),
            ty => ty,
        };
        let same = matches!(
            (&expected, actual),
            (Ty::Int(_), Ty::Int(_))
                | (Ty::Char, Ty::Char)
                | (Ty::Str, Ty::Str)
                | (Ty::Bool, Ty::Bool)
                | (Ty::Float(_), Ty::Float(_))
                | (Ty::Ordering, Ty::Ordering)
        ) || matches!(actual, Ty::Script(..)) && expected == *actual;
        if same {
            Ok(())
        } else {
            Err(Error::TypeMismatch {
                expected: expected.name(&self.models),
                actual: actual.name(&self.models),
                expression: span,
            })
        }
    }

    /// `Some(pattern)` or `None` against `expected`, an `Option<T>`; `Ok(pattern)`
    /// or `Err(pattern)` against a `Result<T, E>`.
    pub(super) fn resolve_prelude_pattern(
        &self,
        syntax: &Syntax,
        expected: &Ty,
        bound_names: &mut BTreeMap<ImmutableString, Span>,
        binding_declarations: &mut Vec<PatternBinding>,
    ) -> Result<(NominalPattern, coverage::Pattern), Error> {
        let Syntax::Prelude {
            variant, payload, ..
        } = syntax
        else {
            unreachable!("only prelude constructor patterns resolve here")
        };
        let expected = self.inference.resolve(expected);
        let element = match (&expected, variant) {
            (Ty::Option(element), Prelude::Some) => Some(&**element),
            (Ty::Option(_), Prelude::None) => None,
            (Ty::Result(ok, _), Prelude::Ok) => Some(&**ok),
            (Ty::Result(_, err), Prelude::Err) => Some(&**err),
            _ => {
                return Err(Error::TypeMismatch {
                    expected: expected.name(&self.models),
                    actual: format!("{}<_>", variant.owner()).into(),
                    expression: syntax.span(),
                })
            }
        };
        // The pattern tests the discriminant; its payload is below it.
        self.trace_test(borrowck::Access::Read, None);
        let (child, coverage) = match (payload, element) {
            (Some(payload), Some(element)) => {
                // `Some`'s and `Ok`'s payload is the first argument, `Err`'s
                // the second.
                let payload_at = match variant {
                    Prelude::Err => borrowck::Step::Arg(1),
                    _ => borrowck::Step::Arg(0),
                };
                let path = self.trace_enter_at(
                    borrowck::Projection::Downcast(variant.name().into()),
                    borrows::TraceRegion::At(borrowck::Path::of(&[payload_at])),
                );
                self.trace_enter_at(
                    borrowck::Projection::Field("0".into()),
                    borrows::TraceRegion::At(borrowck::Path::ROOT),
                );
                let (child, coverage) = self.resolve_child_pattern(
                    payload,
                    element,
                    variant.owner(),
                    variant.name(),
                    bound_names,
                    binding_declarations,
                )?;
                self.trace_leave(path);
                (Some(Box::new(child)), vec![coverage])
            }
            _ => (None, Vec::new()),
        };
        let proof = match (variant, child) {
            (Prelude::Ok, Some(child)) => NominalPattern::Result(Ok(child)),
            (Prelude::Err, Some(child)) => NominalPattern::Result(Err(child)),
            (_, child) => NominalPattern::Option(child),
        };
        Ok((
            proof,
            coverage::Pattern::Constructor {
                key: coverage::Key::Prelude(*variant),
                name: variant.name().into(),
                payload: coverage.into_boxed_slice(),
            },
        ))
    }

    /// The root pattern of a `match` arm, `if let` or `while let` on a value
    /// of `subject` type.
    pub(super) fn resolve_root_pattern(
        &mut self,
        syntax: &Syntax,
        subject: &Ty,
    ) -> Result<ResolvedRoot, Error> {
        let outer_or_types = self.or_binding_types.take();
        let resolved = self.resolve_root_pattern_parts(syntax, subject);
        let or_types = self.or_binding_types.replace(outer_or_types);
        let resolved = resolved?;
        for (first, other, owner, span) in or_types {
            if !(self.unify_type(&first, &other)
                && same_inferred(
                    &self.inference.resolve(&first),
                    &self.inference.resolve(&other),
                ))
            {
                return Err(Error::InvalidMatch {
                    owner: owner.into(),
                    reason: "every alternative of an or-pattern must bind the same names with \
                             the same types"
                        .into(),
                    pattern: span,
                });
            }
        }
        Ok(resolved)
    }

    pub(super) fn resolve_root_pattern_parts(
        &self,
        syntax: &Syntax,
        subject: &Ty,
    ) -> Result<ResolvedRoot, Error> {
        self.binding_mode.set(None);
        let mut bound_names = BTreeMap::<ImmutableString, Span>::new();
        let mut bindings = Vec::new();
        let owner = subject.name(&self.models);
        // What the pattern does to its subject's parts, for the borrow facts.
        let outer = self.begin_trace();
        let resolved = self.resolve_child_pattern(
            syntax,
            subject,
            &owner,
            "the pattern",
            &mut bound_names,
            &mut bindings,
        );
        let trace = self.end_trace(outer);
        let (proof, coverage) = resolved?;
        Ok(ResolvedRoot {
            proof,
            coverage,
            span: syntax.span(),
            bindings,
            trace,
        })
    }

    /// Retain a root pattern's runtime proof; its legacy switch label, which
    /// names the bindings the arm introduces.
    /// The arm label of `proof`, which binds `bindings`: a binding reaching
    /// a part of a `place` by its runtime name.
    pub(super) fn push_root_pattern(
        &mut self,
        proof: NominalPattern,
        bindings: &[PatternBinding],
        place: Option<&PlacePattern>,
    ) -> String {
        let placeholder = pattern_name(self.pattern_offset + self.patterns.len());
        self.patterns.push(proof);
        if bindings.is_empty() {
            format!("{placeholder}::Case")
        } else {
            let names = bindings
                .iter()
                .map(|binding| PlacePattern::runtime_name(place, &binding.name, binding.span))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{placeholder}::Case({names})")
        }
    }

    pub(super) fn resolve_enum_pattern(
        &self,
        syntax: &EnumSyntax,
        expected: &Ty,
        bound_names: &mut BTreeMap<ImmutableString, Span>,
        binding_declarations: &mut Vec<PatternBinding>,
    ) -> Result<ResolvedEnum, Error> {
        let owner: ImmutableString = syntax
            .owner
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join("::")
            .into();
        let model = if syntax.owner.len() == 1 {
            self.named_model(&owner)
        } else {
            self.models
                .qualified_pattern_owner(syntax.owner_span)
                .cloned()
        }
        .ok_or_else(|| Error::UnknownType {
            name: owner.clone(),
            reference: syntax.owner_span,
            path: false,
        })?;
        let Nominal::Enum { definition, item } = model else {
            return Err(Error::InvalidMatch {
                owner,
                reason: "the qualified pattern owner is not an enum".into(),
                pattern: syntax.owner_span,
            });
        };
        let expected = self.inference.resolve(expected);
        let Some(expected_id) = expected.script() else {
            return Err(Error::TypeMismatch {
                expected: expected.name(&self.models),
                actual: definition.name().into(),
                expression: syntax.owner_span,
            });
        };
        if definition.id() != expected_id {
            return Err(Error::TypeMismatch {
                expected: self.models.definition(expected_id).name().into(),
                actual: definition.name().into(),
                expression: syntax.owner_span,
            });
        }
        let Some(variant) = item
            .variants
            .iter()
            .find(|variant| variant.name.text == syntax.variant)
        else {
            return Err(Error::UnknownMember {
                owner: definition.name().into(),
                member: syntax.variant.clone(),
                reference: syntax.variant_span,
            });
        };
        // The written pattern of each payload slot, `_` where `..` skips it.
        let discard = Syntax::Discard {
            span: syntax.variant_span,
        };
        let children = match (&syntax.named, &variant.fields) {
            (Some((written, rest)), declared) => {
                let declared = declared.as_deref().unwrap_or_default();
                if declared.is_empty() && !written.is_empty() {
                    return Err(Error::InvalidMatch {
                        owner: definition.name().into(),
                        reason: format!("variant `{}` has no named fields", variant.name.text)
                            .into(),
                        pattern: syntax.variant_span,
                    });
                }
                // `T { .. }` of a tuple or unit variant matches any payload.
                let rest = *rest || variant.payload.is_empty();
                let names = declared.iter().map(|field| field.text.as_str());
                field_order(definition.name(), names, written, rest, syntax.variant_span)?
                    .into_iter()
                    .map(|index| index.map_or(&discard, |index| &syntax.payload[index]))
                    .chain(std::iter::repeat_n(
                        &discard,
                        if declared.is_empty() {
                            variant.payload.len()
                        } else {
                            0
                        },
                    ))
                    .collect()
            }
            (None, Some(_)) if !variant.payload.is_empty() => {
                return Err(Error::InvalidMatch {
                    owner: definition.name().into(),
                    reason: format!(
                        "`{}::{}` is a struct variant, matched with `{{ .. }}`",
                        definition.name(),
                        variant.name.text
                    )
                    .into(),
                    pattern: syntax.variant_span,
                });
            }
            (None, _) => tuple_members(&syntax.payload, variant.payload.len(), &discard)?,
        };
        if children.len() != variant.payload.len() && !variant.payload.is_empty() {
            let written = syntax
                .payload
                .iter()
                .filter(|member| !matches!(member, Syntax::Rest { .. }))
                .map(Syntax::span)
                .collect::<Vec<_>>();
            let fields = variant
                .payload
                .iter()
                .map(Ty::from_resolved)
                .collect::<Vec<_>>();
            return Err(self.pattern_fields(
                true,
                span(
                    syntax.owner_span.start(),
                    text_end(syntax.variant_span.start(), &syntax.variant),
                ),
                &written,
                &fields,
                definition.id(),
                variant.payload.iter().map(type_ref::Type::span),
            ));
        }
        if children.len() != variant.payload.len() {
            return Err(Error::InvalidMatch {
                owner: definition.name().into(),
                reason: format!(
                    "variant `{}::{}` expects {} binding(s), found {}",
                    definition.name(),
                    variant.name.text,
                    variant.payload.len(),
                    children.len()
                )
                .into(),
                pattern: syntax.variant_span,
            });
        }

        let arguments = match &expected {
            Ty::Script(_, arguments) => arguments.clone(),
            _ => Box::default(),
        };
        let substitution = self.nominal_substitution(definition.id(), &arguments);
        let mut payload = Vec::with_capacity(variant.payload.len());
        let mut coverage_payload = Vec::with_capacity(variant.payload.len());
        // An enum of more than one variant is tested by its discriminant, and
        // the payload is below the variant.
        if item.variants.len() > 1 {
            self.trace_test(borrowck::Access::Read, None);
        }
        let path = self.trace_enter_at(
            borrowck::Projection::Downcast(variant.name.text.clone()),
            borrows::TraceRegion::At(borrowck::Path::ROOT),
        );
        let names = variant.fields.as_deref().unwrap_or_default();
        for (index, (child, expected)) in
            children.into_iter().zip(variant.payload.iter()).enumerate()
        {
            let field = names
                .get(index)
                .map_or_else(|| index.to_string().into(), |name| name.text.clone());
            let field_path = self.trace_enter_at(
                borrowck::Projection::Field(field),
                borrows::TraceRegion::Field(
                    definition.id(),
                    Some(variant.name.text.clone()),
                    index,
                ),
            );
            let (child, coverage) = self.resolve_child_pattern(
                child,
                &instantiate(&Ty::from_resolved(expected), &substitution),
                definition.name(),
                &variant.name.text,
                bound_names,
                binding_declarations,
            )?;
            self.trace_leave(field_path);
            payload.push(child);
            coverage_payload.push(coverage);
        }
        self.trace_leave(path);

        let identity = type_def::Identity::Script(definition.key().clone());
        let variant_id = nominal::variant::Id::from_name(&variant.name);

        Ok(ResolvedEnum {
            proof: NominalEnumPattern {
                enum_name: definition.name().into(),
                identity: identity.clone(),
                schema: definition.schema().clone(),
                variants: item
                    .variants
                    .iter()
                    .map(|variant| variant.name.text.clone())
                    .collect(),
                variant: variant_id.clone(),
                payload: payload.into_boxed_slice(),
            },
            coverage: coverage::Pattern::Constructor {
                key: coverage::Key::Enum {
                    identity,
                    variant: variant_id,
                },
                name: format!("{}::{}", definition.name(), variant.name.text).into(),
                payload: coverage_payload.into_boxed_slice(),
            },
        })
    }

    /// `W(a, b)` in a pattern, of the tuple struct `model`.
    pub(super) fn parse_tuple_struct_pattern(
        &mut self,
        model: Nominal,
        start: Span,
    ) -> Result<Syntax, Error> {
        let (ty, substitution) = self.fresh_nominal(&model, start);
        let Nominal::Struct { definition, item } = model else {
            unreachable!("tuple struct patterns are parsed for structs");
        };
        let fields = item
            .fields
            .iter()
            .map(|field| instantiate(&Ty::from_resolved(&field.ty), &substitution))
            .collect::<Box<[_]>>();
        let mut members = Vec::new();
        let end = if matches!(self.peek(), Token::Unit) {
            self.next().1
        } else {
            self.expect(Token::LeftParen, "'(' after a tuple struct in a pattern")?;
            while !matches!(self.peek(), Token::RightParen | Token::EOF) {
                members.push(self.parse_pattern_syntax()?);
                if matches!(self.peek(), Token::Comma) {
                    self.next();
                } else {
                    break;
                }
            }
            self.expect(Token::RightParen, "')' after tuple struct patterns")?
        };
        let span = span(start.start(), end);
        let written = members
            .iter()
            .filter(|member| !matches!(member, Syntax::Rest { .. }))
            .map(Syntax::span)
            .collect::<Vec<_>>();
        // `W(a, ..)`: `_` for the fields `..` skips.
        if let Some(rest) = tuple_rest(&members)? {
            let fill = (fields.len() + 1).saturating_sub(members.len());
            members.splice(
                rest..=rest,
                std::iter::repeat_with(|| Syntax::Discard { span }).take(fill),
            );
        }
        if members.len() != fields.len() {
            return Err(self.pattern_fields(
                false,
                start,
                &written,
                &fields,
                definition.id(),
                item.fields.iter().map(|field| field.span),
            ));
        }
        Ok(Syntax::TupleStruct {
            ty,
            definition,
            fields,
            members: members.into_boxed_slice(),
            span,
        })
    }

    /// `S { x, y: p, .. }` in a pattern, of the struct `model`: its fields
    /// in declared order, `_` where `..` skips one.
    pub(super) fn parse_struct_pattern(
        &mut self,
        model: Nominal,
        start: Span,
    ) -> Result<Syntax, Error> {
        let (ty, substitution) = self.fresh_nominal(&model, start);
        let Nominal::Struct { definition, item } = model else {
            unreachable!("struct patterns are parsed for structs");
        };
        let (written, rest, end) = self.parse_field_patterns()?;
        let span = span(start.start(), end);
        let names = written
            .iter()
            .map(|(name, name_span, _)| (name.clone(), *name_span))
            .collect::<Vec<_>>();
        let declared = item.fields.iter().map(|field| field.name.text.as_str());
        let order = field_order(definition.name(), declared, &names, rest, span)?;
        let mut written = written
            .into_iter()
            .map(|(_, _, pattern)| Some(pattern))
            .collect::<Vec<_>>();
        let members = order
            .into_iter()
            .map(|index| {
                index
                    .and_then(|index| written[index].take())
                    .unwrap_or(Syntax::Discard { span })
            })
            .collect();
        let fields = item
            .fields
            .iter()
            .map(|field| instantiate(&Ty::from_resolved(&field.ty), &substitution))
            .collect();
        Ok(Syntax::TupleStruct {
            ty,
            definition,
            fields,
            members,
            span,
        })
    }

    /// `{ x, mut y, z: pattern, .. }` after a struct or a struct variant in
    /// a pattern: each field's pattern in written order, whether `..` ends
    /// it, and the position of the `}`.
    #[allow(clippy::type_complexity)]
    pub(super) fn parse_field_patterns(
        &mut self,
    ) -> Result<(Vec<(ImmutableString, Span, Syntax)>, bool, Position), Error> {
        self.expect(Token::LeftBrace, "'{' to start a struct pattern")?;
        let mut fields = Vec::<(ImmutableString, Span, Syntax)>::new();
        let mut rest = false;
        while !matches!(self.peek(), Token::RightBrace | Token::EOF) {
            if matches!(self.peek(), Token::ExclusiveRange) {
                self.next();
                rest = true;
                break;
            }
            if let Some(pattern) = self.parse_ref_binding()? {
                let Syntax::Ref { name, span, .. } = &pattern else {
                    unreachable!("a `ref` binding");
                };
                if fields.iter().any(|(known, ..)| known == name) {
                    return Err(self.unsupported("a field bound twice in one pattern"));
                }
                fields.push((name.clone(), *span, pattern));
                if matches!(self.peek(), Token::Comma) {
                    self.next();
                }
                continue;
            }
            let mutable = matches!(self.peek(), Token::Mut);
            if mutable {
                self.next();
            }
            let (name, name_span) = self.expect_name("a field in a struct pattern")?;
            let pattern = if !mutable && matches!(self.peek(), Token::Colon) {
                self.next();
                self.parse_pattern_syntax()?
            } else {
                Syntax::Binding {
                    name: name.clone(),
                    span: name_span,
                    mutable,
                }
            };
            if fields.iter().any(|(known, ..)| *known == name) {
                return Err(self.unsupported("a field bound twice in one pattern"));
            }
            fields.push((name, name_span, pattern));
            if matches!(self.peek(), Token::Comma) {
                self.next();
            } else if !matches!(self.peek(), Token::RightBrace) {
                return Err(self.unsupported("struct pattern fields without ','"));
            }
        }
        let end = self.expect(Token::RightBrace, "'}' to close a struct pattern")?;
        Ok((fields, rest, end))
    }
}

impl<'a> Parser<'a> {
    /// The pattern syntax of a parameter pattern; a tuple struct's by its
    /// declaration.
    pub(super) fn parameter_pattern_syntax(
        &mut self,
        pattern: &crate::item::ParameterPattern,
    ) -> Result<Syntax, Error> {
        Ok(match pattern {
            crate::item::ParameterPattern::Binding { name, mutable } => Syntax::Binding {
                name: name.text.clone(),
                span: name.span,
                mutable: *mutable,
            },
            crate::item::ParameterPattern::Discard(span) => Syntax::Discard { span: *span },
            crate::item::ParameterPattern::Tuple { members, span } => Syntax::Tuple {
                members: members
                    .iter()
                    .map(|member| self.parameter_pattern_syntax(member))
                    .collect::<Result<_, _>>()?,
                span: *span,
            },
            crate::item::ParameterPattern::Reference {
                mutable,
                pattern,
                span,
            } => Syntax::Reference {
                mutable: *mutable,
                pattern: Box::new(self.parameter_pattern_syntax(pattern)?),
                span: *span,
            },
            // `Some(x): Option<i32>`, which the coverage check refuses as
            // rustc's E0005, as the pattern parser reads the prelude's
            // variants.
            crate::item::ParameterPattern::TupleStruct {
                path,
                members,
                span,
            } if let [name] = &path[..]
                && let [member] = &members[..]
                && let Some(variant) = Prelude::from_name(&name.text)
                && self.models.get(&name.text).is_none() =>
            {
                Syntax::Prelude {
                    variant,
                    payload: Some(Box::new(self.parameter_pattern_syntax(member)?)),
                    span: *span,
                }
            }
            crate::item::ParameterPattern::TupleStruct {
                path,
                members,
                span,
            } => {
                let last = path.last().expect("a tuple struct pattern has a path");
                let model = self
                    .models
                    .qualified_owner(Span::new(path[0].span.start(), last.span.end()))
                    .or_else(|| self.models.get(&last.text))
                    .cloned()
                    .filter(|model| {
                        matches!(model, Nominal::Struct { item, .. }
                            if item.form == crate::item::StructForm::Tuple)
                    });
                // `E::A(x): E`: an enum's variant, which the coverage check
                // finds irrefutable for an enum of one variant, as the
                // pattern parser reads it.
                let Some(model) = model else {
                    let [owner @ .., variant] = &path[..] else {
                        unreachable!("a tuple struct pattern has a path");
                    };
                    let (Some(first), Some(end)) = (owner.first(), owner.last()) else {
                        return Err(Error::UnknownType {
                            name: last.text.clone(),
                            reference: *span,
                            path: false,
                        });
                    };
                    return Ok(Syntax::Enum(EnumSyntax {
                        owner: owner
                            .iter()
                            .map(|name| (name.text.clone(), name.span))
                            .collect(),
                        owner_span: Span::new(
                            first.span.start(),
                            text_end(end.span.start(), &end.text),
                        ),
                        variant: variant.text.clone(),
                        variant_span: variant.span,
                        payload: members
                            .iter()
                            .map(|member| self.parameter_pattern_syntax(member))
                            .collect::<Result<_, _>>()?,
                        named: None,
                        host_struct: false,
                    }));
                };
                let (ty, substitution) = self.fresh_nominal(&model, *span);
                let Nominal::Struct { definition, item } = model else {
                    unreachable!("filtered to a tuple struct");
                };
                let fields = item
                    .fields
                    .iter()
                    .map(|field| instantiate(&Ty::from_resolved(&field.ty), &substitution))
                    .collect::<Box<[_]>>();
                if members.len() != fields.len() {
                    return Err(self.pattern_fields(
                        false,
                        Span::new(path[0].span.start(), last.span.end()),
                        &members
                            .iter()
                            .map(crate::item::ParameterPattern::span)
                            .collect::<Vec<_>>(),
                        &fields,
                        definition.id(),
                        item.fields.iter().map(|field| field.span),
                    ));
                }
                Syntax::TupleStruct {
                    ty,
                    definition,
                    fields,
                    members: members
                        .iter()
                        .map(|member| self.parameter_pattern_syntax(member))
                        .collect::<Result<_, _>>()?,
                    span: *span,
                }
            }
        })
    }

    /// rustc's E0023 for a tuple struct's or, when `variant`, a tuple
    /// variant's pattern, its path at `path`, whose field patterns at
    /// `members` (its `..` aside) are more or fewer than the declared
    /// `fields` of the type `owner`, written at `declared`, which rustc
    /// labels when the pattern's file declares the type.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn pattern_fields(
        &self,
        variant: bool,
        path: Span,
        members: &[Span],
        fields: &[Ty],
        owner: type_def::Id,
        declared: impl Iterator<Item = Span>,
    ) -> Error {
        let declared = if self.models.declared_here(owner) {
            declared.collect()
        } else {
            Box::default()
        };
        let (expected, found) = (fields.len(), members.len());
        let written_owner = self.written(path).unwrap_or_default();
        // The pattern's fields as written, between its parentheses.
        let written = self.written_from(path.end()).and_then(|rest| {
            let rest = rest.trim_start().strip_prefix('(')?;
            let mut depth = 0_usize;
            for (index, character) in rest.char_indices() {
                match character {
                    '(' | '[' | '{' => depth += 1,
                    ')' | ']' | '}' if depth == 0 => return Some(rest[..index].trim().to_string()),
                    ')' | ']' | '}' => depth -= 1,
                    _ => {}
                }
            }
            None
        });
        // From the first field pattern to the end of the last.
        let pattern = match (members.first(), &written) {
            (Some(first), Some(written)) => span(first.start(), text_end(first.start(), written)),
            (Some(first), None) => *first,
            (None, _) => path,
        };
        let tuple = match fields {
            [field] => match self.inference.resolve(field) {
                Ty::Tuple(members) => members.len() == found,
                _ => false,
            },
            _ => false,
        };
        let help = written.and_then(|written| {
            if tuple && found > 0 {
                return Some(format!("missing parentheses: `{written_owner}(({written}))`").into());
            }
            if expected <= found {
                return None;
            }
            let ignored = vec!["_"; expected - found].join(", ");
            let rewritten = if found == 0 {
                ignored
            } else {
                format!("{written}, {ignored}")
            };
            Some(
                format!("use `_` to explicitly ignore each field: `{written_owner}({rewritten})`")
                    .into(),
            )
        });
        Error::PatternFields {
            owner: written_owner.into(),
            variant,
            expected,
            found,
            pattern,
            fields: declared,
            help,
        }
    }

    /// Bind each name a facade pattern makes for the code it guards.
    pub(super) fn bind_host_pattern(&mut self, pattern: &Host) {
        for binding in &pattern.binds {
            self.bindings.insert(
                binding.name.clone(),
                Binding {
                    ty: binding.ty.clone(),
                    mutable: binding.mutable,
                    available: true,
                    declaration: binding.span,
                    kind: BindingKind::Local,
                },
            );
        }
    }

    /// The facade type a pattern path names: `Event`, `keyboard::Event`,
    /// `iced::keyboard::Key`, through the file's imports.
    pub(super) fn host_pattern_owner(&self, owner: &[(ImmutableString, Span)]) -> String {
        let written = owner
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join("::");
        let Some((first, _)) = owner.first() else {
            return written;
        };
        let qualified = match self.models.import(first) {
            Some(binding) => format!(
                "{}{}",
                binding.qualified,
                written.strip_prefix(first.as_str()).unwrap_or_default()
            ),
            None => written,
        };
        self.models.host.canonical_str(&qualified).to_string()
    }

    /// Lower `syntax`, matched against the facade-typed `value` of type `ty`,
    /// into `out`'s tests and bindings. Below a facade enum's variant, its
    /// fields are matched as any pattern on their types.
    pub(super) fn host_pattern(
        &mut self,
        syntax: &Syntax,
        value: origin::Fragment,
        ty: &Ty,
        out: &mut Host,
    ) -> Result<(), Error> {
        // Through `&T` a binding is a reference, Rust's default binding mode;
        // a `&` pattern takes the value itself.
        let (ty, through) = match self.inference.resolve(ty) {
            Ty::Reference { target, mutable } => (self.inference.resolve(&target), Some(mutable)),
            ty => (ty, None),
        };
        let outer = self.host_binding_mode.get();
        let mode = match syntax {
            Syntax::Reference { .. } if through.is_some() => None,
            _ => outer.or(through),
        };
        self.host_binding_mode.set(mode);
        let lowered = self.host_pattern_in_mode(syntax, value, ty, out);
        self.host_binding_mode.set(outer);
        lowered
    }

    pub(super) fn host_pattern_in_mode(
        &mut self,
        syntax: &Syntax,
        value: origin::Fragment,
        ty: Ty,
        out: &mut Host,
    ) -> Result<(), Error> {
        let bound = |ty: &Ty, mode: Option<bool>| match mode {
            Some(mutable) => Ty::Reference {
                mutable,
                target: Box::new(ty.clone()),
            },
            None => ty.clone(),
        };
        match syntax {
            Syntax::Discard { .. } | Syntax::Rest { binding: None, .. } => Ok(()),
            Syntax::Binding {
                name,
                span,
                mutable,
            } => {
                self.require_new_host_binding(out, name, *span)?;
                out.bind(
                    PatternBinding {
                        name: name.clone(),
                        span: *span,
                        ty: bound(&ty, self.host_binding_mode.get()),
                        mutable: *mutable,
                    },
                    value,
                );
                Ok(())
            }
            Syntax::Ref { name, span, .. } => {
                self.require_new_host_binding(out, name, *span)?;
                out.bind(
                    PatternBinding {
                        name: name.clone(),
                        span: *span,
                        ty,
                        mutable: false,
                    },
                    value,
                );
                Ok(())
            }
            Syntax::At {
                name,
                name_span,
                pattern,
            } => {
                self.require_new_host_binding(out, name, *name_span)?;
                out.bind(
                    PatternBinding {
                        name: name.clone(),
                        span: *name_span,
                        ty: bound(&ty, self.host_binding_mode.get()),
                        mutable: false,
                    },
                    value.clone(),
                );
                self.host_pattern(pattern, value, &ty, out)
            }
            Syntax::Reference { pattern, .. } => self.host_pattern(pattern, value, &ty, out),
            Syntax::Literal {
                literal,
                ty: actual,
                span,
            } => {
                self.require_pattern_type(&ty, actual, *span)?;
                let mut test = value.surrounded("(", ")");
                test.push_text(format!(" == {}", scalar_text(literal)));
                out.tests.push(test);
                Ok(())
            }
            Syntax::Range {
                low,
                high,
                inclusive,
                span,
            } => {
                for (_, actual) in low.iter().chain(high.iter()) {
                    if !matches!(actual, Ty::Int(_) | Ty::Char | Ty::Float(_)) {
                        return Err(Error::UnsupportedNominalSyntax {
                            construct: "range pattern".into(),
                            source: *span,
                        });
                    }
                    self.require_pattern_type(&ty, actual, *span)?;
                }
                if let Some((low, _)) = low {
                    let mut test = value.clone().surrounded("(", ")");
                    test.push_text(format!(" >= {}", scalar_text(low)));
                    out.tests.push(test);
                }
                if let Some((high, _)) = high {
                    let operator = if *inclusive { "<=" } else { "<" };
                    let mut test = value.surrounded("(", ")");
                    test.push_text(format!(" {operator} {}", scalar_text(high)));
                    out.tests.push(test);
                }
                Ok(())
            }
            Syntax::Or { alternatives, span } => {
                // Each alternative binds the same names; the first's
                // readers bind them, as a facade reads a field the same
                // way in every variant.
                let mut tests = Vec::new();
                let mut first: Option<Host> = None;
                let mut irrefutable = false;
                for alternative in alternatives.iter() {
                    let mut compiled = Host::default();
                    self.host_pattern(alternative, value.clone(), &ty, &mut compiled)?;
                    let names = |pattern: &Host| {
                        pattern
                            .binds
                            .iter()
                            .map(|binding| binding.name.clone())
                            .collect::<BTreeSet<_>>()
                    };
                    if let Some(first) = &first
                        && names(first) != names(&compiled)
                    {
                        return Err(Error::InvalidMatch {
                            owner: ty.name(&self.models),
                            reason: "each `|` alternative must bind the same names".into(),
                            pattern: *span,
                        });
                    }
                    match compiled.condition() {
                        Some(test) => tests.push(test),
                        None => irrefutable = true,
                    }
                    if first.is_none() {
                        first = Some(compiled);
                    }
                }
                if !irrefutable {
                    let mut joined = origin::Fragment::default();
                    for (index, test) in tests.into_iter().enumerate() {
                        if index > 0 {
                            joined.push_text(" || ");
                        }
                        joined.push(test);
                    }
                    out.tests.push(joined);
                }
                let first = first.expect("an or-pattern has alternatives");
                for binding in &first.binds {
                    self.require_new_host_binding(out, &binding.name, binding.span)?;
                }
                out.absorb_binds(first);
                Ok(())
            }
            Syntax::Prelude {
                variant,
                payload,
                span,
            } => {
                let Ty::Option(element) = &ty else {
                    return Err(Error::TypeMismatch {
                        expected: ty.name(&self.models),
                        actual: variant.name().into(),
                        expression: *span,
                    });
                };
                let element = (**element).clone();
                let test = self.prelude_op(PreludeOp::IsSome, [value.clone()]);
                match (variant, payload) {
                    (Prelude::None, _) => {
                        let mut negated = origin::Fragment::plain("!");
                        negated.push(test);
                        out.tests.push(negated);
                        Ok(())
                    }
                    (Prelude::Some, Some(payload)) => {
                        out.tests.push(test);
                        let inner = self.prelude_op(PreludeOp::Unwrap, [value]);
                        self.host_pattern(payload, inner, &element, out)
                    }
                    _ => Err(Error::TypeMismatch {
                        expected: ty.name(&self.models),
                        actual: variant.name().into(),
                        expression: *span,
                    }),
                }
            }
            Syntax::Enum(pattern) if matches!(ty, Ty::Host { .. }) => {
                self.host_variant_pattern(pattern, value, &ty, out)
            }
            // A unit struct has one value: its pattern tests nothing.
            Syntax::UnitStruct { ty: unit, span } => self.require_type(unit, &ty, *span),
            other => Err(Error::UnsupportedNominalSyntax {
                construct: format!(
                    "this pattern on `{}` below a pattern on a library value",
                    ty.name(&self.models)
                )
                .into(),
                source: other.span(),
            }),
        }
    }

    /// `Owner::Variant`, `Owner::Variant(p)` or `Owner::Variant { field: p, .. }`
    /// on a facade enum value.
    pub(super) fn host_variant_pattern(
        &mut self,
        pattern: &EnumSyntax,
        value: origin::Fragment,
        ty: &Ty,
        out: &mut Host,
    ) -> Result<(), Error> {
        let Ty::Host { proof, .. } = ty else {
            unreachable!("host variant patterns match host values");
        };
        let owner_path = proof.definition.path();
        let written = self.host_pattern_owner(&pattern.owner);
        // A crate type re-exported at another path is the same type, as
        // `iced::Alignment` is `iced::alignment::Alignment`.
        let reexported = || {
            proof.definition.identity().imported().is_some()
                && self
                    .models
                    .host
                    .source_type(&written)
                    .is_some_and(|definition| {
                        definition.identity().imported() == proof.definition.identity().imported()
                    })
        };
        if written != owner_path.as_str() && !reexported() {
            return Err(Error::TypeMismatch {
                expected: owner_path.as_str().into(),
                actual: written.into(),
                expression: pattern.owner_span,
            });
        }
        #[cfg(feature = "rust-import")]
        if !pattern.host_struct
            && proof.definition.identity().imported().is_some()
            && matches!(
                proof.definition.shape(),
                crate::host::type_def::Shape::Enum(_)
            )
        {
            return self.imported_variant_pattern(pattern, value, ty, out);
        }
        // A facade struct's fields, each read and matched on its type.
        if pattern.host_struct {
            let names = pattern
                .named
                .as_ref()
                .map(|(names, _)| names.as_ref())
                .unwrap_or_default();
            for ((field, field_span), member) in names.iter().zip(pattern.payload.iter()) {
                let field_value = self.host_struct_field(value.clone(), ty, field, *field_span)?;
                self.host_pattern(member, field_value.rendered_source(), &field_value.ty, out)?;
            }
            return Ok(());
        }
        let variant = pattern.variant.clone();
        let at = pattern.variant_span;
        let unknown = || Error::UnknownMember {
            owner: owner_path.as_str().into(),
            member: variant.clone(),
            reference: at,
        };
        let variant_path = format!("{owner_path}::{variant}");
        let items = self
            .models
            .host_items(&variant_path)
            .ok_or_else(unknown)?
            .to_vec();
        let bool_ty = crate::host::type_ref::Type::builtin(crate::host::type_ref::Builtin::Bool);
        let reader = items.iter().find(|item| {
            item.kind() == crate::host::item::Kind::Method
                && item.receiver() == Some(receiver::Mode::Shared)
                && item.signature().parameters().is_empty()
        });
        let discards_only = pattern.payload.iter().all(|member| {
            matches!(
                member,
                Syntax::Discard { .. } | Syntax::Rest { binding: None, .. }
            )
        });
        let Some(reader) = reader.cloned() else {
            // A unit variant that is a value: `value == Owner::Variant`.
            let Some(item) = items
                .iter()
                .find(|item| item.kind() == crate::host::item::Kind::AssociatedValue)
                .cloned()
            else {
                return Err(unknown());
            };
            if !pattern.payload.is_empty() {
                return Err(Error::InvalidMatch {
                    owner: owner_path.as_str().into(),
                    reason: format!("`{variant}` is a unit variant").into(),
                    pattern: at,
                });
            }
            let owner = self.models.item_owner(&variant_path).cloned();
            let constant = self.parse_host_value(item, owner, at)?;
            let subject = self.operator_expr(value, ty.clone(), at);
            let test = self.apply_binary(Token::EqualsTo, None, subject, constant)?;
            out.tests.push(test.rendered_source());
            return Ok(());
        };
        let read = self.host_read(value.clone(), ty, variant.clone(), at)?;
        if *reader.signature().output() == bool_ty {
            out.tests.push(read.rendered_source());
            match &pattern.named {
                Some((names, _)) => {
                    for ((field, field_span), member) in names.iter().zip(pattern.payload.iter()) {
                        let field_value =
                            self.host_read(value.clone(), ty, field.clone(), *field_span)?;
                        self.host_pattern(
                            member,
                            field_value.rendered_source(),
                            &field_value.ty,
                            out,
                        )?;
                    }
                }
                None if discards_only => {}
                None => {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: format!("a pattern on `{variant}`'s fields").into(),
                        source: at,
                    });
                }
            }
            return Ok(());
        }
        // A tuple variant of one field: its reader returns it when the
        // value is that variant.
        let Ty::Option(field_ty) = self.inference.resolve(&read.ty) else {
            return Err(unknown());
        };
        let field = read.rendered_source();
        let test = self.prelude_op(PreludeOp::IsSome, [field.clone()]);
        out.tests.push(test);
        // `Variant { .. }` matches a tuple variant whatever its fields, as
        // in Rust.
        if matches!(&pattern.named, Some((names, true)) if names.is_empty()) {
            return Ok(());
        }
        if pattern.named.is_some() {
            return Err(Error::InvalidMatch {
                owner: owner_path.as_str().into(),
                reason: format!("`{variant}` is a tuple variant").into(),
                pattern: at,
            });
        }
        match &*pattern.payload {
            [] if pattern.named.is_none() => Err(Error::InvalidMatch {
                owner: owner_path.as_str().into(),
                reason: format!("`{variant}` is a tuple variant; write `{variant}(..)`").into(),
                pattern: at,
            }),
            [member] => {
                let inner = self.prelude_op(PreludeOp::Unwrap, [field]);
                self.host_pattern(member, inner, &field_ty, out)
            }
            _ => Err(Error::InvalidMatch {
                owner: owner_path.as_str().into(),
                reason: format!("`{variant}` has one field").into(),
                pattern: at,
            }),
        }
    }

    /// `Enum::Variant`, `Enum::Variant(p, ..)` or `Enum::Variant { f: p, .. }`
    /// on an imported Rust enum. The bridge's test tells whether the value
    /// is that variant, and its readers give the variant's fields; each
    /// field is matched as any pattern on its type.
    #[cfg(feature = "rust-import")]
    pub(super) fn imported_variant_pattern(
        &mut self,
        pattern: &EnumSyntax,
        value: origin::Fragment,
        ty: &Ty,
        out: &mut Host,
    ) -> Result<(), Error> {
        let Ty::Host { proof, arguments } = ty else {
            unreachable!("imported variant patterns match imported values");
        };
        let definition = proof.definition.clone();
        let path: ImmutableString = definition.path().as_str().into();
        let crate::host::type_def::Shape::Enum(variants) = definition.shape() else {
            unreachable!("imported variant patterns match enums");
        };
        let at = pattern.variant_span;
        let Some(variant) = variants
            .iter()
            .find(|variant| variant.name() == pattern.variant.as_str())
            .cloned()
        else {
            return Err(Error::UnknownMember {
                owner: path,
                member: pattern.variant.clone(),
                reference: at,
            });
        };
        let mut parameters = definition
            .parameters()
            .iter()
            .cloned()
            .zip(arguments.iter().cloned())
            .collect();
        let mut fields = Vec::with_capacity(variant.fields().len());
        for field in variant.fields() {
            fields.push(self.instantiate_host_type(
                field.ty(),
                &mut parameters,
                &path,
                at,
                proof.owner.as_ref(),
                Some(&definition),
            )?);
        }
        let selector = variant.imported_identity();
        // A matcher reads the value by its handle and gives new values, its
        // fields, which take its output's witness; the value stays
        // observable after it, but for the one that moves the fields out.
        let operation = |kind: crate::host::structural::Kind, output: Ty| ImportedOperation {
            kind,
            operands: vec![(ty.clone(), at, kind != crate::host::structural::Kind::Take)]
                .into_boxed_slice(),
            output,
            receiver: None,
        };
        // The test reads no field, so a field without `Clone` is no
        // obstacle to it.
        let tested = operation(
            crate::host::structural::Kind::Test,
            Ty::Option(Box::new(Ty::Unit)),
        );
        let tester = self.push_imported_op(&path, selector, Some(tested), at)?;
        let test = self.prelude_op(
            PreludeOp::IsSome,
            [origin::Fragment::call(&tester, [value.clone()])],
        );
        out.tests.push(test);
        let rawr_import::surface::identity::Selector::EnumVariant { form, .. } = selector else {
            unreachable!("an enum variant's selector names an enum variant")
        };
        let discard = Syntax::Discard { span: at };
        let members: Vec<(usize, &Syntax)> = match (form, &pattern.named) {
            // `Variant { .. }` matches any variant's fields, as in Rust.
            (_, Some((names, true))) if names.is_empty() => Vec::new(),
            (rawr_import::surface::identity::Form::Named { .. }, Some((names, _))) => {
                let mut members = Vec::with_capacity(names.len());
                for ((name, name_span), member) in names.iter().zip(pattern.payload.iter()) {
                    let Some(index) = variant
                        .fields()
                        .iter()
                        .position(|field| field.name() == name.as_str())
                    else {
                        return Err(Error::UnknownMember {
                            owner: format!("{path}::{}", pattern.variant).into(),
                            member: name.clone(),
                            reference: *name_span,
                        });
                    };
                    members.push((index, member));
                }
                members
            }
            (rawr_import::surface::identity::Form::Tuple { .. }, None)
                if !pattern.payload.is_empty() =>
            {
                let members = tuple_members(&pattern.payload, fields.len(), &discard)?;
                if members.len() != fields.len() {
                    return Err(Error::InvalidMatch {
                        owner: path,
                        reason: format!(
                            "`{}` has {} field(s), the pattern {}",
                            pattern.variant,
                            fields.len(),
                            members.len()
                        )
                        .into(),
                        pattern: at,
                    });
                }
                members.into_iter().enumerate().collect()
            }
            (rawr_import::surface::identity::Form::Unit, None) if pattern.payload.is_empty() => {
                Vec::new()
            }
            _ => {
                return Err(Error::InvalidMatch {
                    owner: path,
                    reason: format!("the pattern does not have `{}`'s form", pattern.variant)
                        .into(),
                    pattern: at,
                })
            }
        };
        // Each field is read alone (#785): one read in place, as Rust reads
        // a `Copy` field and Rawr its own values, is copied, and one a
        // pattern below the variant's matches is lent to it, so a field
        // without `Clone` beside it is no obstacle.
        let reader =
            |this: &mut Self, index: usize, lent: bool| -> Result<origin::Fragment, Error> {
                let field = u32::try_from(index).map_err(|_| Error::InvalidMatch {
                    owner: path.clone(),
                    reason: "the variant has too many fields".into(),
                    pattern: at,
                })?;
                let kind = if lent {
                    crate::host::structural::Kind::Borrow { field }
                } else {
                    crate::host::structural::Kind::Read { field }
                };
                let read = operation(kind, fields[index].clone());
                let helper = this.push_imported_op(&path, selector, Some(read), at)?;
                Ok(origin::Fragment::call(&helper, [value.clone()]))
            };
        // A pattern below the variant's is matched on its field under a
        // hidden name, first, so the variant's own reader can be chosen
        // for what it binds.
        let mut nested = Vec::new();
        for (index, member) in &members {
            if matches!(
                member,
                Syntax::Binding { .. }
                    | Syntax::Ref { .. }
                    | Syntax::Discard { .. }
                    | Syntax::Literal { .. }
                    | Syntax::Range { .. }
            ) {
                continue;
            }
            let name = self.temporary("field");
            let mut below = Host::default();
            self.host_pattern(
                member,
                origin::Fragment::plain(name.as_str()),
                &fields[*index],
                &mut below,
            )?;
            nested.push((*index, name, below));
        }
        // A binding of a field Rawr can't read in place, as an `io::Error`,
        // takes the field as rustc does: by value it moves the variant's
        // fields out, through `&T` or as `ref` it borrows them where they
        // are, a `Clone` field too (#782), and through `&mut T` or as `ref
        // mut` it borrows them mutably (#786). Other fields are read in
        // place, as Rust reads a `Copy` field and Rawr its own values. A
        // pattern below that moves or mutably borrows reads its field from
        // the variant's fields moved or mutably borrowed alike.
        let native = |this: &Self, ty: &Ty| {
            !this.type_provides(ty, capability::Kind::Copy) && !this.plain_value(ty)
        };
        let outer = self.host_binding_mode.get();
        // Whether a binding of a native field has the binding mode `wanted`:
        // `None` by value, else `Some` of whether it is mutable.
        let binds = |wanted: Option<bool>| {
            members.iter().any(|(index, member)| {
                let mode = match member {
                    Syntax::Binding { .. } => outer,
                    Syntax::Ref { mutable, .. } => Some(*mutable),
                    _ => return false,
                };
                mode == wanted && native(self, &fields[*index])
            })
        };
        let mutable = binds(Some(true)) || nested.iter().any(|(.., below)| below.lends_mutably());
        let moves = binds(None) || nested.iter().any(|(.., below)| below.moves());
        let lends = binds(Some(false));
        let kind = if mutable {
            Some(crate::host::structural::Kind::LendMut)
        } else if moves {
            Some(crate::host::structural::Kind::Take)
        } else if lends {
            Some(crate::host::structural::Kind::Lend)
        } else {
            None
        };
        let part = match kind {
            Some(kind) => {
                let output = Ty::Tuple(fields.clone().into_boxed_slice());
                // A guard binds what the arm moves by reference.
                let guard = if kind == crate::host::structural::Kind::Take {
                    let lent = operation(crate::host::structural::Kind::Lend, output.clone());
                    let lender = self.push_imported_op(&path, selector, Some(lent), at)?;
                    Some(origin::Fragment::call(&lender, [value.clone()]))
                } else {
                    None
                };
                let read =
                    self.push_imported_op(&path, selector, Some(operation(kind, output)), at)?;
                let part = HostPart::fields(
                    self.temporary("part"),
                    origin::Fragment::call(&read, [value.clone()]),
                    guard,
                    kind == crate::host::structural::Kind::LendMut,
                );
                Some(out.part(part))
            }
            None => None,
        };
        let mut nested = nested.into_iter().peekable();
        for (index, member) in members {
            if let Some((_, name, below)) = nested.next_if(|(field, ..)| *field == index) {
                // The pattern's tests read the field lent to them alone.
                if let Some(condition) = below.condition() {
                    let mut test = origin::Fragment::plain(format!("{{ let {name} = "));
                    test.push(reader(self, index, true)?);
                    test.push_text("; ");
                    test.push(condition);
                    test.push_text(" }");
                    out.tests.push(test);
                }
                if below.binds.is_empty() {
                    continue;
                }
                // What it binds reads the field from the variant's part
                // when the part moves the fields or lends them mutably, and
                // borrowed alone else, past the scope too (#783).
                let held = part.filter(|part| out.parts[*part].holds());
                let field = match held {
                    Some(held) => {
                        let mut field = origin::Fragment::plain(out.parts[held].name.as_str());
                        field.push_text(format!("[{index}]"));
                        let source = &out.parts[held];
                        HostPart {
                            name,
                            escaping: source.escaping.is_some().then(|| field.clone()),
                            reader: field,
                            guard: None,
                            moves: source.moves,
                            mutable: source.mutable,
                        }
                    }
                    None => {
                        let borrowed = reader(self, index, true)?;
                        HostPart {
                            name,
                            reader: borrowed.clone(),
                            guard: None,
                            escaping: Some(borrowed),
                            moves: false,
                            mutable: false,
                        }
                    }
                };
                out.part(field);
                for binding in &below.binds {
                    self.require_new_host_binding(out, &binding.name, binding.span)?;
                }
                out.absorb_binds(below);
                continue;
            }
            if matches!(member, Syntax::Discard { .. }) {
                continue;
            }
            let copy = reader(self, index, false)?;
            let Some(part) =
                part.filter(|_| matches!(member, Syntax::Binding { .. } | Syntax::Ref { .. }))
            else {
                self.host_pattern(member, copy, &fields[index], out)?;
                continue;
            };
            // The binding any pattern on the field makes, read from the
            // part instead: by reference for `ref` and `ref mut`.
            let mut single = Host::default();
            self.host_pattern(member, copy, &fields[index], &mut single)?;
            let (Some(mut binding), Some(copy)) = (single.binds.pop(), single.values.pop()) else {
                unreachable!("a binding pattern binds its name")
            };
            if let Syntax::Ref { mutable, .. } = member {
                binding.ty = Ty::Reference {
                    mutable: *mutable,
                    target: Box::new(binding.ty),
                };
            }
            self.require_new_host_binding(out, &binding.name, binding.span)?;
            out.bind_part(binding, part, index, copy);
        }
        Ok(())
    }
}
