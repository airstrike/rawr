//! `std::cell::RefCell<T>`, Rust's cell whose value a shared reference may
//! change. At runtime it is a `Cell`, whose copies share one value, as a
//! moved or borrowed `RefCell` is one cell in Rust.
//!
//! A borrow is a `Guard`, which the cell counts until it is released.
//! `let mut v = cell.borrow_mut();` keeps its guard in a hidden local, so
//! the borrow lasts until the local's block ends, as Rust's `RefMut` does.
//! A borrow in an expression is released at the end of its statement. A
//! borrow that conflicts with one still held is Rust's panic.

use super::*;

#[cfg(not(feature = "sync"))]
type BorrowToken = std::rc::Weak<()>;
#[cfg(feature = "sync")]
type BorrowToken = std::sync::Weak<()>;

/// A `RefCell`: its value and the borrows it holds.
#[derive(Clone)]
pub struct Cell(crate::Shared<Inner>);

pub struct Inner {
    value: crate::Locked<Dynamic>,
    /// Whether this is a `Cell`, which `Debug` names, not a `RefCell`.
    plain: bool,
    /// Each held borrow's token and whether it is mutable. A borrow whose
    /// guard is gone is released.
    borrows: crate::Locked<Vec<(BorrowToken, bool)>>,
}

/// One borrow of a cell, held while any copy of it lives.
#[derive(Clone)]
pub(super) struct Guard {
    cell: crate::Shared<Inner>,
    token: crate::Shared<()>,
}

impl Inner {
    fn value(&self) -> Dynamic {
        crate::func::locked_read(&self.value).map_or(Dynamic::UNIT, |value| value.clone())
    }

    fn set(&self, value: Dynamic) -> Dynamic {
        crate::func::locked_write(&self.value).map_or(Dynamic::UNIT, |mut held| {
            std::mem::replace(&mut *held, value)
        })
    }

    /// Whether a borrow is held, and whether a held one is mutable.
    fn held(&self) -> (bool, bool) {
        let Some(mut borrows) = crate::func::locked_write(&self.borrows) else {
            return (false, false);
        };
        borrows.retain(|(token, _)| token.strong_count() > 0);
        (
            !borrows.is_empty(),
            borrows.iter().any(|(_, mutable)| *mutable),
        )
    }
}

fn panic(message: &str) -> RawrResult {
    Err(EvalAltResult::ErrorRuntime(message.into(), Position::NONE).into())
}

fn cell(value: &Dynamic) -> Option<crate::Shared<Inner>> {
    if let Some(cell) = value.read_lock::<Cell>() {
        return Some(cell.0.clone());
    }
    value.read_lock::<Guard>().map(|guard| guard.cell.clone())
}

/// The value of a cell or a borrow, for `==`.
pub(super) fn value(value: &Dynamic) -> Option<Dynamic> {
    cell(value).map(|cell| cell.value())
}

/// The cell's type name and value, for `Debug`.
pub(super) fn named_value(value: &Dynamic) -> Option<(&'static str, Dynamic)> {
    cell(value).map(|cell| {
        let name = if cell.plain { "Cell" } else { "RefCell" };
        (name, cell.value())
    })
}

/// A new cell of `value`, a `Cell` when `plain`.
fn made(value: Dynamic, plain: bool) -> Dynamic {
    Dynamic::from(Cell(crate::Shared::new(Inner {
        value: crate::Locked::new(value),
        plain,
        borrows: crate::Locked::new(Vec::new()),
    })))
}

/// A new `RefCell` of `value`.
pub(super) fn new(value: Dynamic) -> Dynamic {
    made(value, false)
}

/// Whether the cell `value` is borrowed; `None` when it is no cell.
pub(super) fn held(value: &Dynamic) -> Option<bool> {
    let cell = value.read_lock::<Cell>()?.0.clone();
    Some(cell.held().0)
}

/// A mutable borrow of the cell `value` when nothing borrows it.
pub(super) fn try_borrow_mut(value: &Dynamic) -> Option<Dynamic> {
    let inner = value.read_lock::<Cell>()?.0.clone();
    if inner.held().0 {
        return None;
    }
    let token = crate::Shared::new(());
    if let Some(mut borrows) = crate::func::locked_write(&inner.borrows) {
        borrows.push((crate::Shared::downgrade(&token), true));
    }
    Some(Dynamic::from(Guard { cell: inner, token }))
}

pub(super) fn invoke(op: PreludeOp, args: &mut FnCallArgs) -> RawrResult {
    if matches!(op, PreludeOp::CellNew | PreludeOp::PlainCellNew) {
        return Ok(made(args[0].take(), matches!(op, PreludeOp::PlainCellNew)));
    }
    // An imported method's borrow guard: each read and store reaches the
    // native target through one call of its own, so there is no borrow to
    // release.
    #[cfg(feature = "rust-import")]
    {
        let place = args[0]
            .read_lock::<crate::island::place::Place>()
            .map(|place| place.clone());
        if let Some(place) = place {
            return match op {
                PreludeOp::CellGet => place.get(),
                PreludeOp::CellSet => place.set(args[1].take()).map(|_| Dynamic::UNIT),
                PreludeOp::CellRelease => Ok(Dynamic::UNIT),
                _ => Err(invariant("a borrow guard's place is read and stored only")),
            };
        }
    }
    let inner = cell(args[0]).ok_or_else(|| invariant("a `RefCell` is not one at runtime"))?;
    match op {
        PreludeOp::CellBorrow => {
            let mutable = args[1].as_bool().unwrap_or(false);
            let (held, held_mutably) = inner.held();
            if mutable && held {
                return panic("already borrowed: BorrowMutError");
            }
            if held_mutably {
                return panic("already mutably borrowed: BorrowError");
            }
            let token = crate::Shared::new(());
            if let Some(mut borrows) = crate::func::locked_write(&inner.borrows) {
                borrows.push((crate::Shared::downgrade(&token), mutable));
            }
            Ok(Dynamic::from(Guard { cell: inner, token }))
        }
        PreludeOp::CellGet => Ok(inner.value()),
        PreludeOp::CellSet => {
            let _ = inner.set(args[1].take());
            Ok(Dynamic::UNIT)
        }
        PreludeOp::CellRelease => {
            if let Some(guard) = args[0].read_lock::<Guard>()
                && let Some(mut borrows) = crate::func::locked_write(&inner.borrows)
            {
                let held = crate::Shared::as_ptr(&guard.token);
                borrows.retain(|(token, _)| token.as_ptr() != held);
            }
            Ok(Dynamic::UNIT)
        }
        // `replace` and `take` need the cell unborrowed, as Rust's do.
        PreludeOp::CellReplace => {
            if inner.held().0 {
                return panic("already borrowed: BorrowMutError");
            }
            Ok(inner.set(args[1].take()))
        }
        PreludeOp::CellClone => {
            if inner.held().1 {
                return panic("already mutably borrowed: BorrowError");
            }
            Ok(made(inner.value(), inner.plain))
        }
        _ => Err(invariant("not a `RefCell` operation")),
    }
}

impl Parser<'_> {
    /// Which cell the path `path` names in a body: `RefCell` by any of its
    /// spellings, and `Cell` by its module path or as imported from
    /// `std::cell`, unless the program names a type so.
    pub(super) fn cell_named(&self, path: &str) -> Option<CellKind> {
        if crate::resolve::std_cell(path) && self.models.get("RefCell").is_none() {
            return Some(CellKind::RefCell);
        }
        let imported = || {
            self.models
                .import(path)
                .is_some_and(|binding| crate::resolve::std_plain_cell(&binding.qualified))
        };
        (self.models.get("Cell").is_none()
            && (crate::resolve::std_plain_cell(path) || path == "Cell" && imported()))
        .then_some(CellKind::Cell)
    }

    /// How many tokens after `name` spell the path to `RefCell` or `Cell`
    /// before its `::new` or `::default` (`RefCell`, `cell::RefCell` or
    /// `std::cell::RefCell`), and which cell it is.
    pub(super) fn cell_path(&mut self, name: &str) -> Option<(usize, CellKind)> {
        if self.bindings.contains_key(name) {
            return None;
        }
        let names = self.lookahead_path(name);
        let (count, kind) = (1..=names.len()).find_map(|count| {
            self.cell_named(&names[..count].join("::"))
                .map(|kind| (count, kind))
        })?;
        matches!(
            names.get(count).map(String::as_str),
            Some("new" | "default")
        )
        .then_some(((count - 1) * 2, kind))
    }

    /// `RefCell<T>` or `Cell<T>` in a body, after the name.
    pub(super) fn parse_cell_type(&mut self, kind: CellKind) -> Result<Ty, Error> {
        self.expect(Token::LessThan, "'<' after the cell's name")?;
        let inner = self.parse_local_type()?;
        self.expect_close_angle("'>' after the cell's type argument")?;
        Ok(Ty::Cell(kind, Box::new(inner)))
    }

    /// `RefCell::new(value)` or `RefCell::default()`, and `Cell`'s, after
    /// the name.
    pub(super) fn parse_cell_constructor(
        &mut self,
        name_span: Span,
        kind: CellKind,
    ) -> Result<ParsedExpr, Error> {
        self.expect(Token::DoubleColon, "'::' after the cell's name")?;
        let (member, member_span) = self.expect_member_name()?;
        let (mut arguments, end) = self.parse_call_arguments("cell constructor arguments")?;
        let call = span(name_span.start(), end);
        match (member.as_str(), arguments.as_mut_slice()) {
            ("new", [value]) => {
                let ty = Ty::Cell(kind, Box::new(self.inference.resolve(&value.ty)));
                // A cell of a value with drop glue is refused.
                self.needs_drop(&ty, call)?;
                let source = self.prelude_op(kind.new_op(), [value.rendered_source()]);
                Ok(self.operator_expr(source, ty, call))
            }
            ("default", []) => {
                let inner = self.inference.fresh();
                let ty = Ty::Cell(kind, Box::new(inner.clone()));
                self.pending_bounds
                    .push((inner, capability::Kind::Default, member_span));
                let source = self.default_source(&ty, member_span)?;
                Ok(self.operator_expr(source, ty, call))
            }
            _ => Err(Error::InvalidMethodCall {
                owner: kind.name().into(),
                method: member,
                reason: format!(
                    "is not a `{}` constructor Rawr has; it has `new` and `default`",
                    kind.name()
                )
                .into(),
                call,
            }),
        }
    }

    /// `cell.borrow()`, `borrow_mut()`, `get_mut()`, `into_inner()`,
    /// `replace(v)`, `take()` and `clone()`.
    pub(super) fn parse_cell_member(
        &mut self,
        value: ParsedExpr,
        inner: Ty,
        member: ImmutableString,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let (mut arguments, end) = self.parse_call_arguments("`RefCell` method arguments")?;
        let call = span(value.span.start(), end);
        let cell = value.rendered_source();
        match (member.as_str(), arguments.as_mut_slice()) {
            ("borrow" | "borrow_mut" | "get_mut", []) => {
                let mutable = member.as_str() != "borrow";
                let flag: origin::Fragment = if mutable { "true" } else { "false" }.into();
                let borrow = self.prelude_op(PreludeOp::CellBorrow, [cell.clone(), flag]);
                // A change borrows the cell where the place is read
                // (`PlaceStep::Cell`), so its value is read without a second
                // borrow; a shared borrow is checked where it is read.
                let source = if mutable {
                    self.prelude_op(PreludeOp::CellGet, [cell])
                } else {
                    self.prelude_op(PreludeOp::CellGet, [borrow.clone()])
                };
                let mut expression = self.operator_expr(
                    source,
                    Ty::Reference {
                        mutable,
                        target: Box::new(inner.clone()),
                    },
                    call,
                );
                // A change through a borrow in an expression: the borrow
                // made before the statement and released after it.
                if mutable {
                    let place = self.temporary("cell");
                    self.bindings.insert(
                        place.as_str().into(),
                        Binding {
                            ty: Ty::Unknown,
                            mutable: true,
                            available: true,
                            moved_fields: Default::default(),
                            declaration: call,
                            kind: BindingKind::Local,
                        },
                    );
                    expression.lvalue = Some(Lvalue::Nested(Box::new(NestedLvalue {
                        binding: place.as_str().into(),
                        base: borrow.clone(),
                        steps: vec![PlaceStep::Cell { release: true }],
                        ty: inner.clone(),
                    })));
                }
                self.last_cell_borrow = Some(CellBorrow {
                    span: call,
                    borrow,
                    mutable,
                    inner,
                });
                Ok(expression)
            }
            ("into_inner", []) => {
                let source = self.prelude_op(PreludeOp::CellGet, [cell]);
                Ok(self.operator_expr(source, inner, call))
            }
            ("replace", [replacement]) => {
                self.coerce_expr(&inner, replacement)?;
                let source = self.prelude_op(
                    PreludeOp::CellReplace,
                    [cell, replacement.rendered_source()],
                );
                Ok(self.operator_expr(source, inner, call))
            }
            ("take", []) => {
                if !self.type_provides(&inner, capability::Kind::Default) {
                    return Err(Error::MissingCapability {
                        owner: inner.name(&self.models),
                        capability: "Default".into(),
                        operation: member_span,
                    });
                }
                let default = self.default_source(&inner, member_span)?;
                let source = self.prelude_op(PreludeOp::CellReplace, [cell, default]);
                Ok(self.operator_expr(source, inner, call))
            }
            ("clone", []) => {
                let source = self.prelude_op(PreludeOp::CellClone, [cell]);
                Ok(self.operator_expr(source, Ty::Cell(CellKind::RefCell, Box::new(inner)), call))
            }
            _ => Err(Error::UnknownMember {
                owner: format!("RefCell<{}>", inner.name(&self.models)).into(),
                member,
                reference: member_span,
            }),
        }
    }

    /// Refuses `clone()` of a value of `ty` that holds a cell of its own:
    /// Rawr's copy would share the cell, where Rust's clone makes a new
    /// one. A cell's own `clone()` makes one.
    pub(super) fn refuse_cell_clone(&self, ty: &Ty, at: Span) -> Result<(), Error> {
        let ty = match self.inference.resolve(ty) {
            Ty::Reference { target, .. } => *target,
            ty => ty,
        };
        if self.models.holds_cell(&ty, &mut Vec::new()) {
            return Err(Error::UnsupportedNominalSyntax {
                construct: format!(
                    "`clone()` of a `{}`, which holds a `Cell` or `RefCell`",
                    ty.name(&self.models)
                )
                .into(),
                source: at,
            });
        }
        Ok(())
    }

    /// `cell.get()`, `set(v)`, `replace(v)`, `take()`, `swap(&other)`,
    /// `into_inner()` and `clone()` of a `Cell`, whose value is copied in
    /// and out and never borrowed.
    pub(super) fn parse_plain_cell_member(
        &mut self,
        value: ParsedExpr,
        inner: Ty,
        member: ImmutableString,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let (mut arguments, end) = self.parse_call_arguments("`Cell` method arguments")?;
        let call = span(value.span.start(), end);
        let cell = value.rendered_source();
        let needs = |this: &Self, capability: capability::Kind, name: &str| {
            if this.type_provides(&inner, capability) {
                Ok(())
            } else {
                Err(Error::MissingCapability {
                    owner: inner.name(&this.models),
                    capability: name.into(),
                    operation: member_span,
                })
            }
        };
        match (member.as_str(), arguments.as_mut_slice()) {
            ("get", []) => {
                needs(self, capability::Kind::Copy, "Copy")?;
                let source = self.prelude_op(PreludeOp::CellGet, [cell]);
                Ok(self.operator_expr(source, inner, call))
            }
            ("into_inner", []) => {
                let source = self.prelude_op(PreludeOp::CellGet, [cell]);
                Ok(self.operator_expr(source, inner, call))
            }
            ("set", [replacement]) => {
                self.coerce_expr(&inner, replacement)?;
                let source =
                    self.prelude_op(PreludeOp::CellSet, [cell, replacement.rendered_source()]);
                Ok(self.operator_expr(source, Ty::Unit, call))
            }
            ("replace", [replacement]) => {
                self.coerce_expr(&inner, replacement)?;
                let source = self.prelude_op(
                    PreludeOp::CellReplace,
                    [cell, replacement.rendered_source()],
                );
                Ok(self.operator_expr(source, inner, call))
            }
            ("take", []) => {
                needs(self, capability::Kind::Default, "Default")?;
                let default = self.default_source(&inner, member_span)?;
                let source = self.prelude_op(PreludeOp::CellReplace, [cell, default]);
                Ok(self.operator_expr(source, inner, call))
            }
            // `a.swap(&b)`: each takes the other's value.
            ("swap", [other]) => {
                let expected = Ty::Reference {
                    mutable: false,
                    target: Box::new(Ty::Cell(CellKind::Cell, Box::new(inner.clone()))),
                };
                self.require_type(&expected, &other.ty, other.span)?;
                let theirs = self.prelude_op(PreludeOp::CellGet, [other.rendered_source()]);
                let ours = self.prelude_op(PreludeOp::CellReplace, [cell, theirs]);
                let source = self.prelude_op(PreludeOp::CellSet, [other.rendered_source(), ours]);
                Ok(self.operator_expr(source, Ty::Unit, call))
            }
            ("clone", []) => {
                needs(self, capability::Kind::Copy, "Copy")?;
                let source = self.prelude_op(PreludeOp::CellClone, [cell]);
                Ok(self.operator_expr(source, Ty::Cell(CellKind::Cell, Box::new(inner)), call))
            }
            _ => Err(Error::UnknownMember {
                owner: format!("Cell<{}>", inner.name(&self.models)).into(),
                member,
                reference: member_span,
            }),
        }
    }

    /// rawr-iced's tokio `mutex.lock().await`, after `lock`: the mutex is a
    /// `RefCell` at runtime. The task of `tokio::__lock` waits until no
    /// borrow holds it and gives the mutable borrow it then makes, which the
    /// expression is, as `borrow_mut()` is.
    pub(super) fn parse_mutex_lock(
        &mut self,
        mutex: ParsedExpr,
        inner: Ty,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let (arguments, _) = self.parse_call_arguments("`lock` arguments")?;
        let awaited = arguments.is_empty()
            && matches!(self.peek(), Token::Period)
            && matches!(self.peek_second(), Token::Reserved(ref keyword) if keyword.as_str() == "await");
        if !awaited || !self.closures.last().is_some_and(|scope| scope.asynchronous) {
            return Err(Error::UnsupportedNominalSyntax {
                construct:
                    "a tokio `Mutex` locked other than by `mutex.lock().await` in async code".into(),
                source: span(mutex.span.start(), member_span.end()),
            });
        }
        self.next();
        let end = self.next().1;
        let call = span(mutex.span.start(), end);
        let items = self
            .models
            .host_items("rawr_iced::tokio::__lock")
            .map(<[_]>::to_vec)
            .ok_or_else(|| self.unsupported("tokio's `Mutex` without rawr-iced"))?;
        let owner = self.models.item_owner("rawr_iced::tokio::__lock").cloned();
        let cell = mutex.rendered_source();
        let lock = self.call_host_function(&items, owner, vec![mutex], call)?;
        let mut borrow = origin::Fragment::plain(format!("{}(", crate::eval::machine::AWAIT));
        borrow.push(lock.rendered_source());
        borrow.push_text(")");
        // As for `borrow_mut()`, a change locks where the place is read, and
        // a read in an expression (`*mutex.lock().await + 1`) reads the value
        // without waiting, so `+=` does not lock twice.
        let source = self.prelude_op(PreludeOp::CellGet, [cell]);
        let mut expression = self.operator_expr(
            source,
            Ty::Reference {
                mutable: true,
                target: Box::new(inner.clone()),
            },
            call,
        );
        let place = self.temporary("cell");
        self.bindings.insert(
            place.as_str().into(),
            Binding {
                ty: Ty::Unknown,
                mutable: true,
                available: true,
                moved_fields: Default::default(),
                declaration: call,
                kind: BindingKind::Local,
            },
        );
        expression.lvalue = Some(Lvalue::Nested(Box::new(NestedLvalue {
            binding: place.as_str().into(),
            base: borrow.clone(),
            steps: vec![PlaceStep::Cell { release: true }],
            ty: inner.clone(),
        })));
        self.last_cell_borrow = Some(CellBorrow {
            span: call,
            borrow,
            mutable: true,
            inner,
        });
        Ok(expression)
    }

    /// `let v = cell.borrow_mut();`: the borrow kept in a hidden local for
    /// `v`'s block, and `v` the borrowed value, read and stored through it.
    pub(super) fn bind_cell_borrow(
        &mut self,
        name: &ImmutableString,
        name_span: Span,
        mutable: bool,
        borrow: CellBorrow,
    ) -> origin::Fragment {
        let guard = self.temporary("borrow");
        self.cell_guards.insert(
            name.clone(),
            (
                name_span,
                guard.clone(),
                borrow.mutable,
                borrow.inner.clone(),
            ),
        );
        self.bindings.insert(
            name.clone(),
            Binding {
                ty: Ty::Reference {
                    mutable: borrow.mutable,
                    target: Box::new(borrow.inner),
                },
                mutable,
                available: true,
                moved_fields: Default::default(),
                declaration: name_span,
                kind: BindingKind::Local,
            },
        );
        let mut rendered = origin::Fragment::plain(format!("let {guard} = "));
        rendered.push(borrow.borrow);
        rendered.push_text(";");
        rendered
    }

    /// A use of a local bound to a borrow: the cell's value, a place a
    /// change stores back into the cell.
    pub(super) fn cell_guard_use(
        &mut self,
        name: &ImmutableString,
        at: Span,
    ) -> Option<ParsedExpr> {
        let declaration = self.bindings.get(name.as_str())?.declaration;
        let (_, guard, mutable, inner) = self
            .cell_guards
            .get(name)
            .filter(|(guard_declaration, ..)| *guard_declaration == declaration)?
            .clone();
        let source = self.prelude_op(PreludeOp::CellGet, [guard.as_str().into()]);
        let mut value = self.operator_expr(
            source,
            Ty::Reference {
                mutable,
                target: Box::new(inner.clone()),
            },
            at,
        );
        if mutable {
            value.lvalue = Some(Lvalue::Nested(Box::new(NestedLvalue {
                binding: name.clone(),
                base: origin::Fragment::plain(guard),
                steps: vec![PlaceStep::Cell { release: false }],
                ty: inner,
            })));
        }
        Some(value)
    }

    /// One cell step of a nested place: the statements reading the
    /// borrowed value into `local` (when `reads`) and storing it back.
    pub(super) fn cell_step(
        &mut self,
        place: &origin::Fragment,
        release: bool,
        reads: bool,
    ) -> (origin::Fragment, origin::Fragment, String) {
        let guard = self.temporary("guard");
        let local = self.temporary("cellvalue");
        let set = self.push_op(Op::Prelude(PreludeOp::CellSet));
        let mut read = origin::Fragment::plain(format!("let {guard} = "));
        read.push(place.clone());
        if reads {
            let get = self.push_op(Op::Prelude(PreludeOp::CellGet));
            read.push_text(format!("; let {local} = {get}({guard}); "));
        } else {
            // The statement overwrites the value, so it isn't read.
            read.push_text(format!("; let {local} = (); "));
        }
        let mut store = origin::Fragment::plain(format!("{set}({guard}, {local}); "));
        if release {
            let release = self.push_op(Op::Prelude(PreludeOp::CellRelease));
            store.push_text(format!("{release}({guard}); "));
        }
        (read, store, local)
    }
}

/// The borrow an expression just made, which a `let` may keep.
#[derive(Clone)]
pub(super) struct CellBorrow {
    pub(super) span: Span,
    pub(super) borrow: origin::Fragment,
    pub(super) mutable: bool,
    pub(super) inner: Ty,
}
