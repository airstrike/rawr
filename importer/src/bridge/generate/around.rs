//! A value whose type holds an opaque type inside a type of the crate's
//! own, as a function's `Wrapper<impl SomeTrait>` does, and the operands
//! of other types that hold the value's type, as a `Pipe<Signal<Wrapper<
//! impl SomeTrait>>>` a call takes beside it.
//!
//! The table of calls that take such a value is compiled where rustc knows
//! the value's type, and its witness names that type (`take_like`). One
//! table's source serves each value a call gives, as each of a tuple's
//! members, so the witness's type is the one of the value it is made for
//! (`witnessed`). An operand of another type that holds the opaque type is
//! taken at its own type: a function generic over the opaque type makes
//! that type's witness of the value's. Each type it names may bound its
//! parameters, as `Signal<S: SomeTrait>` does, so the function's parameter
//! has the bounds the opaque type's declaration records, which the opaque
//! type meets.

use super::{convert, request, Operation, Plan};

/// Where a table's source names the type of the value it is made for, as
/// a shape takes it (`witnessed`).
const WITNESSED: &str = "__RAWR_WITNESSED__";

/// The bounds the declaration of the opaque type a value holds records,
/// and the types of the value's that hold the opaque type itself.
pub(super) struct Around {
    bounds: String,
    /// Each type of the value's whose argument is the opaque type, as
    /// `Wrapper<_>` of a `Signal<Wrapper<impl SomeTrait>>`: its Rust
    /// spelling, and the same with `T` there. A `_` outside them is a type
    /// rustc infers from the call, as `Start<_>` of an associated type the
    /// call fixes, not the opaque type. Empty where the opaque type is in
    /// no such type, where each `_` is it.
    holders: Vec<(String, String)>,
}

impl Around {
    /// The type `returned` of the value `plan` gives, around the opaque
    /// type `key`, whose declaration bounds it in the signature of the call
    /// of `cohort` that gives it: `plan` itself, or another, as a generic
    /// `pipe<T>()` gives a pipe of what `wrapped(..)` gave. None where
    /// `returned` is the opaque type itself, holds another, or no call has
    /// bounds of it with a Rust spelling.
    pub(super) fn of(
        plan: &Plan,
        cohort: &[Plan],
        returned: &convert::Type,
        key: &convert::Type,
    ) -> Result<Option<Self>, request::Error> {
        if super::same_opaque(returned, key) || !super::only_opaque_holes(returned, key) {
            return Ok(None);
        }
        let convert::Type::Nominal { definition, .. } = key else {
            return Ok(None);
        };
        for candidate in std::iter::once(plan).chain(cohort) {
            let (Operation::Function {
                signature,
                substitutions,
                ..
            }
            | Operation::Method {
                signature,
                substitutions,
                ..
            }) = candidate.operation()
            else {
                continue;
            };
            if !signature
                .opaques()
                .iter()
                .any(|opaque| opaque.definition() == definition.as_bytes())
            {
                continue;
            }
            if let Some(bounds) = super::declared_bounds(signature, substitutions, key)? {
                let mut holders = Vec::new();
                holders_of(returned, key, &mut holders);
                return Ok(Some(Self { bounds, holders }));
            }
        }
        Ok(None)
    }

    /// `body` with each operand at `position` of a type that holds the
    /// opaque type taken at that type, and each argument native code lent
    /// of such a type reached at it: the witness of the value's type, `_w`,
    /// made the witness of the operand's.
    pub(super) fn shape(&self, body: &str, position: usize) -> String {
        let take = format!("take_like(arguments, {position}, _w)?");
        let mut shaped = String::with_capacity(body.len());
        for line in body.split_inclusive('\n') {
            let line = self.lent(line);
            let Some(holding) =
                super::taken_type(&line, position, &take).and_then(|ty| self.generic(ty))
            else {
                shaped.push_str(&line);
                continue;
            };
            shaped.push_str(&line.replacen(
                &take,
                &format!(
                    "take_like(arguments, {position}, {})?",
                    self.witness(&holding)
                ),
                1,
            ));
        }
        self.annotated(&shaped)
    }

    /// `body`, of a call that takes no operand of a type that holds the
    /// opaque type but gives one, as a generic `pipe::<Signal<_>>()` or a
    /// variant of an enum at that type does: each value it reaches and each
    /// `let` of such a type at the witness's type, and the value it gives
    /// declared `given`, the Rust type of what the call gives, where it
    /// writes none.
    pub(super) fn made(&self, body: &str, given: Option<&str>) -> String {
        let body = match given {
            Some(given) => body.replacen("let output = ", &format!("let output: {given} = "), 1),
            None => body.to_owned(),
        };
        let mut lent = String::with_capacity(body.len());
        for line in body.split_inclusive('\n') {
            lent.push_str(&self.lent(line));
        }
        self.annotated(&lent)
    }

    /// The Rust type `ty` with `T` at each place it holds the opaque type,
    /// in the types that hold it itself (`holders`). None where it holds
    /// none.
    fn generic(&self, ty: &str) -> Option<String> {
        if self.holders.is_empty() {
            return generic(ty);
        }
        let mut generic = ty.to_owned();
        for (spelled, at_t) in &self.holders {
            generic = generic.replace(spelled.as_str(), at_t);
        }
        (generic != ty).then_some(generic)
    }

    /// `body` with each `let name: Type = value;` of a type that holds the
    /// opaque type, whose `_` rustc can't infer from the value alone, made
    /// `let name: Type = witnessed_as(witness, value);`.
    fn annotated(&self, body: &str) -> String {
        let mut annotated = String::with_capacity(body.len());
        let mut rest = body;
        while let Some(at) = rest.find("let ") {
            let after = &rest[at + 4..];
            let starts = rest[..at]
                .chars()
                .next_back()
                .is_none_or(|before| before.is_whitespace() || before == '{' || before == ';');
            let declared = starts
                .then(|| {
                    let colon = after.find(": ")?;
                    let name = &after[..colon];
                    let name = name.strip_prefix("mut ").unwrap_or(name);
                    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                        return None;
                    }
                    let ty_at = colon + 2;
                    let ty_end = depth_zero(&after[ty_at..], " = ")?;
                    // A declaration without a value ends first.
                    if depth_zero(&after[ty_at..], ";").is_some_and(|end| end < ty_end) {
                        return None;
                    }
                    let value_at = ty_at + ty_end + 3;
                    let value_end = depth_zero(&after[value_at..], ";")?;
                    let holding = self.generic(&after[ty_at..ty_at + ty_end])?;
                    Some((value_at, value_at + value_end, holding))
                })
                .flatten();
            let Some((value_at, value_end, holding)) = declared else {
                annotated.push_str(&rest[..at + 4]);
                rest = after;
                continue;
            };
            annotated.push_str(&rest[..at + 4]);
            annotated.push_str(&after[..value_at]);
            annotated.push_str(&format!(
                "witnessed_as({}, {})",
                self.witness(&holding),
                &after[value_at..value_end]
            ));
            rest = &after[value_end..];
        }
        annotated.push_str(rest);
        annotated
    }

    /// `line` with each value it reaches at a type that holds the opaque
    /// type reached at that type through the witness ([`REACHES`]).
    fn lent(&self, line: &str) -> String {
        let mut lent = line.to_owned();
        for reach in REACHES {
            lent = self.reached(&lent, &reach);
        }
        lent
    }

    /// `line` with each of `reach`'s turbofish calls of a type that holds
    /// the opaque type written through the witness.
    fn reached(&self, line: &str, reach: &Reach) -> String {
        let Reach {
            turbofish,
            after,
            inferred,
            like,
        } = reach;
        let mut reached = String::with_capacity(line.len());
        let mut rest = line;
        while let Some(at) = rest.find(turbofish) {
            reached.push_str(&rest[..at]);
            let tail = &rest[at + turbofish.len()..];
            let mut depth = 1_usize;
            let end = tail.char_indices().find_map(|(index, character)| {
                match character {
                    '<' => depth += 1,
                    '>' => depth -= 1,
                    _ => {}
                }
                (depth == 0).then_some(index)
            });
            let holding = end
                .filter(|end| tail[end + 1..].starts_with(after))
                .and_then(|end| {
                    let ty = &tail[..end];
                    let ty = if *inferred {
                        ty.strip_suffix(", _")?
                    } else {
                        ty
                    };
                    Some((end, self.generic(ty)?))
                });
            let Some((end, holding)) = holding else {
                reached.push_str(&rest[at..at + turbofish.len()]);
                rest = tail;
                continue;
            };
            reached.push_str(&like.replace("{w}", &self.witness(&holding)));
            rest = &tail[end + 1 + after.len()..];
        }
        reached.push_str(rest);
        reached
    }

    /// The witness of `holding`, a type with `T` at each opaque position,
    /// made of the value's, `_w`, by a function generic over the opaque
    /// type, of its declaration's bounds.
    fn witness(&self, holding: &str) -> String {
        // Each borrow's elided lifetime is one of the function's own, but
        // inside parentheses, where a `fn(&T)` or `dyn FnMut(&T)` takes it
        // at any lifetime.
        let mut elided = String::with_capacity(holding.len());
        let mut parenthesized = 0_usize;
        for (index, character) in holding.char_indices() {
            elided.push(character);
            match character {
                '(' => parenthesized += 1,
                ')' => parenthesized = parenthesized.saturating_sub(1),
                '&' if parenthesized == 0 && !holding[index + 1..].starts_with('\'') => {
                    elided.push_str("'_ ");
                }
                _ => {}
            }
        }
        let (holding, lifetimes) = super::named_lifetimes(&elided);
        let bounds = &self.bounds;
        format!(
            "{{ fn shape<{lifetimes}T: {bounds} + 'static>(_: std::marker::PhantomData<{WITNESSED}>) -> std::marker::PhantomData<{holding}> {{ std::marker::PhantomData }} shape(_w) }}"
        )
    }
}

/// A call generic over the type of a value it reaches, `{turbofish}Type>`
/// followed by `after`, with `, _` after the type where `inferred`: where
/// the type holds the opaque type, the call is written `like`, the same
/// call at the type its witness, `{w}`, names.
struct Reach {
    turbofish: &'static str,
    after: &'static str,
    inferred: bool,
    like: &'static str,
}

/// The calls a table's entry makes that reach a value at its type.
const REACHES: [Reach; 11] = [
    Reach {
        turbofish: "arguments.arena.held::<",
        after: "(",
        inferred: true,
        like: "held_like(arguments.arena, {w}, ",
    },
    Reach {
        turbofish: "arguments.arena.share::<",
        after: "(",
        inferred: false,
        like: "share_like(arguments.arena, {w}, ",
    },
    Reach {
        turbofish: "reach_lent::<",
        after: "",
        inferred: false,
        like: "reach_lent_like({w})",
    },
    Reach {
        turbofish: "value.shared::<",
        after: "()",
        inferred: false,
        like: "value.shared_like({w})",
    },
    Reach {
        turbofish: "value.mutable::<",
        after: "()",
        inferred: false,
        like: "value.mutable_like({w})",
    },
    Reach {
        turbofish: "__rawr_internal::place::reach_as::<",
        after: "(",
        inferred: true,
        like: "reach_as_like({w}, ",
    },
    Reach {
        turbofish: "arguments.arena.held_shared::<",
        after: "(",
        inferred: true,
        like: "held_shared_like(arguments.arena, {w}, ",
    },
    Reach {
        turbofish: "arguments.arena.shared::<",
        after: "(",
        inferred: true,
        like: "shared_at(arguments.arena, {w}, ",
    },
    Reach {
        turbofish: "arguments.arena.mutable::<",
        after: "(",
        inferred: true,
        like: "mutable_at(arguments.arena, {w}, ",
    },
    Reach {
        turbofish: "reach_some::<",
        after: "",
        inferred: false,
        like: "reach_some_like({w})",
    },
    Reach {
        turbofish: "lends::<",
        after: "()",
        inferred: false,
        like: "lends_like({w})",
    },
];

/// `table`, a table's source, made for a value of `ty`: each shape takes
/// the witness of `ty` with `T` at each of its opaque positions. A type
/// Rust can't spell that way leaves the shape `T`, which rustc refuses.
pub(super) fn witnessed(table: &str, ty: &convert::Type) -> String {
    if !table.contains(WITNESSED) {
        return table.to_owned();
    }
    let spelled = super::concrete_type(ty)
        .filter(|spelled| !spelled.contains('\''))
        .and_then(|spelled| generic(&spelled))
        .unwrap_or_else(|| "T".to_owned());
    table.replace(WITNESSED, &spelled)
}

/// Where `text` first has `end` outside any bracket, brace, parenthesis
/// or string literal. None where a closing one comes first.
fn depth_zero(text: &str, end: &str) -> Option<usize> {
    let mut depth = 0_usize;
    let mut quoted = false;
    let mut escaped = false;
    for (index, character) in text.char_indices() {
        if quoted {
            match character {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => quoted = false,
                _ => {}
            }
            continue;
        }
        if depth == 0 && text[index..].starts_with(end) {
            return Some(index);
        }
        match character {
            '"' => quoted = true,
            '(' | '[' | '{' | '<' => depth += 1,
            '>' if text[..index].ends_with(['-', '=']) => {}
            ')' | ']' | '}' | '>' => depth = depth.checked_sub(1)?,
            _ => {}
        }
    }
    None
}

/// The Rust type `ty` with `T` at each type it leaves to inference, `_`,
/// which holds the opaque type. None where it leaves none.
fn generic(ty: &str) -> Option<String> {
    let holes = super::opaque_holes(ty);
    if holes.is_empty() {
        return None;
    }
    let mut generic = String::with_capacity(ty.len());
    let mut from = 0;
    for hole in holes {
        generic.push_str(&ty[from..hole]);
        generic.push('T');
        from = hole + 1;
    }
    generic.push_str(&ty[from..]);
    Some(generic)
}

/// Each type in `ty` with the opaque type `key` among its arguments, as
/// [`Around::holders`] spells it.
fn holders_of(ty: &convert::Type, key: &convert::Type, holders: &mut Vec<(String, String)>) {
    match ty {
        convert::Type::Nominal { arguments, .. } => {
            let opaque = arguments
                .iter()
                .map(|argument| super::same_opaque(argument, key))
                .collect::<Vec<_>>();
            if opaque.contains(&true)
                && let Some(spelled) = super::concrete_type(ty)
                && let Some(at_t) = at_arguments(&spelled, &opaque)
                && !holders.iter().any(|(known, _)| *known == spelled)
            {
                holders.push((spelled, at_t));
            }
            for argument in arguments {
                holders_of(argument, key, holders);
            }
        }
        convert::Type::Tuple(members) => {
            for member in members {
                holders_of(member, key, holders);
            }
        }
        convert::Type::Option(inner)
        | convert::Type::Borrowed { ty: inner, .. }
        | convert::Type::Range(inner)
        | convert::Type::Sequence { element: inner, .. } => holders_of(inner, key, holders),
        convert::Type::Result(ok, err) => {
            holders_of(ok, key, holders);
            holders_of(err, key, holders);
        }
        _ => {}
    }
}

/// `spelled`, a nominal type's Rust spelling `Path<'_, A, B>`, with `T`
/// at each of its type arguments `opaque` marks, after the lifetimes it
/// writes first. None where its arguments aren't spelled that way.
fn at_arguments(spelled: &str, opaque: &[bool]) -> Option<String> {
    let open = spelled.find('<')?;
    let inner = spelled.strip_suffix('>')?.get(open + 1..)?;
    let mut parts = Vec::new();
    let mut rest = inner;
    while let Some(end) = depth_zero(rest, ", ") {
        parts.push(&rest[..end]);
        rest = &rest[end + 2..];
    }
    parts.push(rest);
    let lifetimes = parts.len().checked_sub(opaque.len())?;
    let mut written = Vec::with_capacity(parts.len());
    for (index, part) in parts.iter().enumerate() {
        let at_opaque = index
            .checked_sub(lifetimes)
            .is_some_and(|argument| opaque[argument]);
        if at_opaque {
            if *part != "_" {
                return None;
            }
            written.push("T");
        } else {
            written.push(part);
        }
    }
    Some(format!("{}<{}>", &spelled[..open], written.join(", ")))
}
