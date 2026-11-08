//! Patterns and the bindings they make: their syntax, and patterns resolved
//! against script and crate types.

use super::coverage::CoveragePattern;
use super::ty::Ty;
use super::{borrows, origin, text_end};
use crate::ast::{NominalEnumPattern, NominalPattern};
use crate::{ast, type_def, ImmutableString, Span};

#[derive(Clone)]
pub(super) enum PatternSyntax {
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
        members: Box<[PatternSyntax]>,
        span: Span,
    },
    Enum(EnumPatternSyntax),
    /// `Some(pattern)` or `None` on an `Option<T>`, `Ok(pattern)` or
    /// `Err(pattern)` on a `Result<T, E>`.
    Prelude {
        variant: Prelude,
        payload: Option<Box<PatternSyntax>>,
        span: Span,
    },
    /// `Cow::Borrowed(pattern)` or `Cow::Owned(pattern)` (`owned`) on a
    /// `Cow<'_, B>`.
    Cow {
        owned: bool,
        payload: Box<PatternSyntax>,
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
        members: Box<[PatternSyntax]>,
        span: Span,
    },
    /// `p | q`.
    Or {
        alternatives: Box<[PatternSyntax]>,
        span: Span,
    },
    /// `[a, b, rest @ .., z]` on an array or slice; `rest` is the `..`
    /// element (`Rest`) when written.
    Slice {
        before: Box<[PatternSyntax]>,
        rest: Option<Box<PatternSyntax>>,
        after: Box<[PatternSyntax]>,
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
        pattern: Box<PatternSyntax>,
        span: Span,
    },
    /// `name @ pattern`.
    At {
        name: ImmutableString,
        name_span: Span,
        pattern: Box<PatternSyntax>,
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

impl PatternSyntax {
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
pub(super) struct EnumPatternSyntax {
    pub(super) owner: SpannedNames,
    pub(super) owner_span: Span,
    pub(super) variant: ImmutableString,
    pub(super) variant_span: Span,
    pub(super) payload: Box<[PatternSyntax]>,
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
pub(in crate::resolve) enum PatternSite {
    /// `let pattern = value;`.
    Let,
    /// `for pattern in values`.
    For,
    /// A function's parameter, `fn f(E::A(x): E)`.
    Argument,
}

pub(super) struct ResolvedEnumPattern {
    pub(super) proof: NominalEnumPattern,
    pub(super) coverage: CoveragePattern,
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
pub(super) struct HostPattern {
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

/// One binding or part of a `HostPattern`, by its index.
#[derive(Clone, Copy)]
pub(super) enum HostStep {
    Bind(usize),
    #[cfg_attr(
        not(feature = "rust-import"),
        expect(dead_code, reason = "only imported variants have parts")
    )]
    Part(usize),
}

/// Where a `HostPattern`'s bindings are read.
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

impl HostPattern {
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
pub(super) struct ResolvedRootPattern {
    pub(super) proof: NominalPattern,
    pub(super) coverage: CoveragePattern,
    pub(super) span: Span,
    pub(super) bindings: Vec<PatternBinding>,
    /// What the pattern does to its subject's parts (`borrows.rs`).
    pub(super) trace: borrows::PatternTrace,
}
