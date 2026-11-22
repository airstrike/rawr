//! `thread_local!` keys and std's `LocalKey<T>`. A key the macro declares
//! where an item goes is a constant of `LocalKey<T>`, as std's expansion
//! declares it, and one in a body is a block-scoped item (`block_static`).
//! A use of either is the key's runtime value (`eval::thread_local`): its
//! key, the initializer that makes its value at its first access on a
//! thread, and the glue that drops a value of a type with drop glue as its
//! thread ends.
//!
//! Each method is std's, as std defines it over `with`: `with_borrow` and
//! `with_borrow_mut` borrow the `RefCell` and call the closure, `get`,
//! `take`, `replace` and `update` run on the cell, and `set` makes the
//! key's cell itself where the thread has no value yet, without running
//! the initializer. A closure's `&mut` parameter is copied in and back out
//! (`copied_parameters`).

use super::*;

impl Parser<'_> {
    /// The runtime value of the key `key`, whose initializer `initializer`
    /// renders a `T` of: `thread(|| initializer, local_key, [key, glue])`,
    /// `glue` the drop of a value of a type with drop glue.
    pub(super) fn local_key(
        &mut self,
        key: &str,
        initializer: origin::Fragment,
        ty: &Ty,
        at: Span,
    ) -> Result<origin::Fragment, Error> {
        let glue = if self.needs_drop(ty, at)? {
            let shape = self.drop_shape(ty);
            format!("(|__rawr_value| {}(__rawr_value, {shape}))", drops::GLUE)
        } else {
            "()".into()
        };
        let operand = origin::Fragment::plain(format!("[{key:?}, {glue}]"));
        Ok(self.prelude_op(
            PreludeOp::Thread,
            [
                initializer.surrounded("(|| {\n", "\n})"),
                thread::code("local_key"),
                operand,
            ],
        ))
    }

    /// The statement that ends the thread's thread-local values as `main`
    /// returns, as std's exit runs the main thread's destructors, written
    /// after its body's drops. Only a crate with `impl Drop` writes it:
    /// without one, nothing a value's end does is seen.
    pub(super) fn end_main_thread(&mut self, definition: &item::Function) {
        let main = definition.receiver.is_none()
            && self.callable.as_ref().is_some_and(|key| {
                key.owner().is_none() && key.modules().is_empty() && key.name() == "main"
            });
        if !main || !self.drops() {
            return;
        }
        let end = self.prelude_op(
            PreludeOp::Thread,
            ["()".into(), thread::code("thread_end"), "()".into()],
        );
        let mut statement = end.as_str().to_owned();
        statement.push_str("; ");
        self.end_thread_after_scope(statement);
    }

    /// A method of a `LocalKey<T>`, its receiver `value` the key or a
    /// reference to it: std's, else rustc's E0599 for one std declares for
    /// no `LocalKey` of this `T`.
    pub(super) fn parse_local_key_member(
        &mut self,
        value: ParsedExpr,
        inner: Ty,
        member: ImmutableString,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let owner = Ty::Thread(thread::Kind::LocalKey, Box::new(inner.clone()));
        let cell = match self.inference.resolve(&inner) {
            Ty::Cell(kind, inside) => Some((kind, *inside)),
            _ => None,
        };
        // Of the impls that declare the member (`set` of `LocalKey<Cell<T>>`
        // and of `LocalKey<RefCell<T>>`), the one whose self type fits.
        let found = std_method::inherent(&owner, &member);
        if found.is_none() {
            return Err(Error::UnknownMember {
                owner: owner.name(&self.models),
                member,
                reference: member_span,
            });
        }
        let local = self.prelude_op(
            PreludeOp::Thread,
            [value.rendered_source(), thread::code("local"), "()".into()],
        );
        let at = value.span;
        let borrowed = |this: &Self| {
            let target = Box::new(inner.clone());
            this.operator_expr(
                local.clone(),
                Ty::Reference {
                    mutable: false,
                    target,
                },
                at,
            )
        };
        match (member.as_str(), cell) {
            ("with", _) => self.parse_local_key_with(value, local, inner, &member),
            ("with_borrow" | "with_borrow_mut", Some((CellKind::RefCell, inside))) => {
                self.parse_local_key_borrow(value, local, inside, &member)
            }
            ("set", Some((kind, inside))) => self.parse_local_key_set(value, kind, inside),
            // `self.with(|cell| cell.get())`, as std defines each.
            ("get" | "take" | "replace" | "update", Some((CellKind::Cell, inside))) => {
                let cell = borrowed(self);
                self.parse_plain_cell_member(cell, inside, member, member_span)
            }
            ("take" | "replace", Some((CellKind::RefCell, inside))) => {
                let cell = borrowed(self);
                self.parse_cell_member(cell, inside, member, member_span)
            }
            _ => Err(match found {
                Some(found) => Error::NotYet {
                    path: std_method::path(&found).into(),
                    reference: member_span,
                },
                None => Error::UnknownMember {
                    owner: owner.name(&self.models),
                    member,
                    reference: member_span,
                },
            }),
        }
    }

    /// The one argument of the `LocalKey` method `member`, read with std's
    /// declaration of it, and the call's span.
    fn local_key_argument(
        &mut self,
        value: &ParsedExpr,
        member: &str,
    ) -> Result<(ParsedExpr, Span), Error> {
        let (mut arguments, end) =
            self.parse_std_arguments(&value.ty, member, "`LocalKey` method arguments")?;
        let call = span(value.span.start(), end);
        if arguments.len() != 1 {
            return Err(Error::argument_count(
                Callee::Method,
                crate::resolve::Qualified("LocalKey", member).to_string(),
                1,
                arguments.len(),
                call,
            ));
        }
        Ok((arguments.remove(0), call))
    }

    /// `key.with(f)`: `f` called with the thread's value, made at its first
    /// access here.
    fn parse_local_key_with(
        &mut self,
        value: ParsedExpr,
        local: origin::Fragment,
        inner: Ty,
        member: &str,
    ) -> Result<ParsedExpr, Error> {
        let (mut function, call) = self.local_key_argument(&value, member)?;
        let input = Ty::Reference {
            mutable: false,
            target: Box::new(inner),
        };
        let output = self.inference.fresh();
        let source = self.callable_call(&mut function, &[input], &output, vec![local])?;
        self.move_local(&mut function)?;
        let output = self.inference.resolve(&output);
        Ok(self.operator_expr(source, output, call))
    }

    /// `key.with_borrow(f)` and `key.with_borrow_mut(f)` of a
    /// `LocalKey<RefCell<T>>`: the cell borrowed while `f` runs with its
    /// value, as `self.with(|cell| f(&cell.borrow()))` does. A mutable
    /// borrow's value is copied in and back out of `f`.
    fn parse_local_key_borrow(
        &mut self,
        value: ParsedExpr,
        local: origin::Fragment,
        inside: Ty,
        member: &str,
    ) -> Result<ParsedExpr, Error> {
        let mutable = member == "with_borrow_mut";
        let (mut function, call) = self.local_key_argument(&value, member)?;
        let cell = self.temporary("cell");
        let guard = self.temporary("borrow");
        let returned = self.temporary("returned");
        let flag: origin::Fragment = if mutable { "true" } else { "false" }.into();
        let borrow = self.prelude_op(PreludeOp::CellBorrow, [cell.as_str().into(), flag]);
        let read = if mutable {
            self.prelude_op(PreludeOp::CellGet, [cell.as_str().into()])
        } else {
            self.prelude_op(PreludeOp::CellGet, [guard.as_str().into()])
        };
        let input = Ty::Reference {
            mutable,
            target: Box::new(inside),
        };
        let output = self.inference.fresh();
        let called = self.callable_call(&mut function, &[input], &output, vec![read])?;
        self.move_local(&mut function)?;
        let release = self.prelude_op(PreludeOp::CellRelease, [guard.as_str().into()]);
        let moved = crate::engine::KEYWORD_MOVE;
        let mut source = origin::Fragment::plain(format!("{{ let {cell} = "));
        source.push(local);
        source.push_text(format!("; let {guard} = "));
        source.push(borrow);
        source.push_text(format!("; let {returned} = "));
        source.push(called);
        source.push_text("; ");
        let value = if mutable {
            let stored = format!("{moved}({returned}[1])");
            let store = self.prelude_op(
                PreludeOp::CellSet,
                [cell.as_str().into(), origin::Fragment::plain(stored)],
            );
            source.push(store);
            source.push_text("; ");
            format!("{moved}({returned}[0])")
        } else {
            returned
        };
        source.push(release);
        source.push_text(format!("; {value} }}"));
        let output = self.inference.resolve(&output);
        Ok(self.operator_expr(source, output, call))
    }

    /// `key.set(v)` of a `LocalKey<Cell<T>>` or `LocalKey<RefCell<T>>`:
    /// where the thread has no value yet, its cell is made of `v` without
    /// running the initializer, as std's `initialize_with` does; else `v`
    /// replaces the cell's value, which drops.
    fn parse_local_key_set(
        &mut self,
        value: ParsedExpr,
        kind: CellKind,
        inside: Ty,
    ) -> Result<ParsedExpr, Error> {
        let (mut replacement, call) = self.local_key_argument(&value, "set")?;
        self.coerce_expr(&inside, &mut replacement)?;
        self.move_local(&mut replacement)?;
        let key = self.temporary("key");
        let given = self.temporary("value");
        let replaced = self.temporary("replaced");
        let vacant = self.prelude_op(
            PreludeOp::Thread,
            [key.as_str().into(), thread::code("vacant"), "()".into()],
        );
        let new = match kind {
            CellKind::Cell => PreludeOp::PlainCellNew,
            _ => PreludeOp::CellNew,
        };
        let made = self.prelude_op(new, [given.as_str().into()]);
        let put = self.prelude_op(
            PreludeOp::Thread,
            [key.as_str().into(), thread::code("put"), made],
        );
        let local = self.prelude_op(
            PreludeOp::Thread,
            [key.as_str().into(), thread::code("local"), "()".into()],
        );
        let replace = self.prelude_op(PreludeOp::CellReplace, [local, given.as_str().into()]);
        let dropped = if self.needs_drop(&inside, call)? {
            drops::glue_call(&replaced, &self.drop_shape(&inside))
        } else {
            String::new()
        };
        let mut source = origin::Fragment::plain(format!("{{ let {key} = "));
        source.push(value.rendered_source());
        source.push_text(format!("; let {given} = "));
        source.push(replacement.rendered_source());
        source.push_text("; if ");
        source.push(vacant);
        source.push_text(" { ");
        source.push(put);
        source.push_text(format!("; }} else {{ let {replaced} = "));
        source.push(replace);
        source.push_text(format!("; {dropped} }} }}"));
        Ok(self.operator_expr(source, Ty::Unit, call))
    }
}
