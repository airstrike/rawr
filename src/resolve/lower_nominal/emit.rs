//! Emitting operations into the lowered text: private calls of pushed
//! operations, operands evaluated once, and placeholders written over.

use super::*;

impl<'a> Parser<'a> {
    /// Write `text` over `placeholder`, padded to its length so source
    /// locations stay exact, in the body and its lifted closures. Whether
    /// the placeholder was written yet: a call still being read isn't.
    pub(super) fn write_over(&mut self, placeholder: &str, text: &str) -> bool {
        let replacement = format!("{text:<width$}", width = placeholder.len());
        debug_assert_eq!(replacement.len(), placeholder.len());
        let mut found = self.output.replace_same_length(placeholder, &replacement);
        for closure in &mut self.lifted {
            found |= closure.body.replace_same_length(placeholder, &replacement);
        }
        found
    }

    pub(super) fn private_call(
        &mut self,
        helper: &str,
        operands: impl IntoIterator<Item = ParsedExpr>,
    ) -> (origin::Fragment, origin::Fragment) {
        // Keep compiler-private legacy source shallow without changing the user's
        // Engine limits. Each pushed operation owns one unique helper spelling,
        // so its operand indexes also form collision-free temporary names.
        if !self.normalize_private_calls {
            // Legacy switch guards reject statement-block expressions. Retain
            // their earlier inline shape so guarded work stays branch-local.
            let operands = operands
                .into_iter()
                .map(|operand| {
                    debug_assert!(operand.prelude.is_empty());
                    operand.source
                })
                .collect::<Vec<_>>();
            return (origin::Fragment::default(), self.op_call(helper, operands));
        }
        let (prelude, operands) = self.private_operands(helper, operands);
        (prelude, self.op_call(helper, operands))
    }

    /// Each operand of a pushed operation evaluated once, in order, into a
    /// local of its own.
    pub(super) fn private_operands(
        &mut self,
        helper: &str,
        operands: impl IntoIterator<Item = ParsedExpr>,
    ) -> (origin::Fragment, Vec<origin::Fragment>) {
        let mut prelude = origin::Fragment::default();
        let operands = operands
            .into_iter()
            .enumerate()
            .map(|(index, operand)| {
                self.materialize(operand, &format!("{helper}_arg_{index}"), &mut prelude)
            })
            .collect::<Vec<_>>();
        (prelude, operands)
    }

    /// The call of a pushed operation. A host operation in a body whose
    /// signature names a generic inside a host type also takes those
    /// generics' witnesses, which instantiate its proof at run time.
    pub(super) fn op_call(
        &mut self,
        helper: &str,
        operands: impl IntoIterator<Item = origin::Fragment>,
    ) -> origin::Fragment {
        let mut operands = operands.into_iter().collect::<Vec<_>>();
        if self.host_witnesses.is_empty() {
            return origin::Fragment::call(helper, operands);
        }
        let index = helper
            .strip_prefix(SENTINEL)
            .and_then(|index| index.strip_prefix(self.numbering.as_str()))
            .and_then(|index| index.parse::<usize>().ok())
            .and_then(|index| index.checked_sub(self.op_offset));
        if let Some(index) = index
            && matches!(
                self.ops.get(index),
                Some(Op::Intrinsic { .. } | Op::Projection { .. })
            )
        {
            self.capture_witnesses();
            operands.extend(
                self.host_witnesses
                    .iter()
                    .map(|name| origin::Fragment::plain(witness_variable(name))),
            );
            self.witnessed_ops
                .insert(index, self.host_witnesses.clone().into_boxed_slice());
        }
        origin::Fragment::call(helper, operands)
    }

    pub(super) fn materialize(
        &mut self,
        operand: ParsedExpr,
        temporary: &str,
        prelude: &mut origin::Fragment,
    ) -> origin::Fragment {
        // Invocation authority may only remain the exact current formal. It must
        // never acquire an alias, even in compiler-private generated source.
        if operand.ty.invocation().is_some() {
            debug_assert!(
                operand.prelude.is_empty(),
                "invocation authority must remain an unadorned current formal"
            );
            return operand.rendered_source();
        }
        prelude.push(operand.prelude);
        let mut statement = origin::Fragment::plain(format!("let {temporary} = "));
        statement.push(operand.source);
        statement.push_text(";\n");
        prelude.push(statement);
        // The operand's local is the call's alone: a value that moves goes
        // into the call, as Rust moves it, and the local keeps no copy of
        // it (#698). A `vec![Family::new(..)]` given to a crate function
        // then gives the function the families themselves. So does a
        // `&mut PathBuf` a crate's call takes moved in (`moved_input`).
        if self.moves_by_value(&operand.ty) || moved_input(&self.inference, &operand.ty) {
            return origin::Fragment::call(
                crate::engine::KEYWORD_MOVE,
                [origin::Fragment::plain(temporary)],
            );
        }
        origin::Fragment::plain(temporary)
    }

    pub(super) fn materialize_in_place(
        &mut self,
        operand: &mut ParsedExpr,
        temporary: &str,
    ) -> origin::Fragment {
        if operand.ty.invocation().is_some() {
            let rendered = operand.rendered_source();
            operand.prelude.clear();
            return rendered;
        }
        let source = std::mem::take(&mut operand.source);
        let mut statement = origin::Fragment::plain(format!("let {temporary} = "));
        statement.push(source);
        statement.push_text(";\n");
        operand.prelude.push(statement);
        origin::Fragment::plain(temporary)
    }

    pub(super) fn push_op(&mut self, op: Op) -> String {
        let name = format!(
            "{SENTINEL}{}{}",
            self.numbering,
            self.op_offset + self.ops.len()
        );
        self.ops.push(op);
        name
    }
}
