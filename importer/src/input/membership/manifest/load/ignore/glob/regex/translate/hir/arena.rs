// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../LICENSE-regex-syntax-MIT.
//! Flat ownership for consuming structural HIR construction.
use super::{Expr, Kind, Properties};
use crate::input::membership::manifest::{session::Session, storage::Buffer, Error, Step};
use rawr_toml::document::Admission;
mod alternation;
mod concat;
mod equal;
mod singleton;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in super::super) struct Id(usize);
pub(super) struct Arena(Buffer<Option<Expr>>);
impl Arena {
    pub(super) fn new() -> Self {
        Self(Buffer::new())
    }
    fn retain<A: Admission>(&mut self, child: Expr, session: &mut Session<'_, A>) -> Step<Id> {
        Ok(Id(self.0.push(Some(child), session)?))
    }
    pub(super) fn get<A: Admission>(&self, id: Id, session: &mut Session<'_, A>) -> Step<&Expr> {
        session.items(1)?;
        match self.0.as_slice().get(id.0).and_then(Option::as_ref) {
            Some(expr) => Ok(expr),
            None => session.fail(Error::State),
        }
    }
    fn take<A: Admission>(&mut self, id: Id, session: &mut Session<'_, A>) -> Step<Expr> {
        session.items(1)?;
        match self.0.as_mut_slice().get_mut(id.0).and_then(Option::take) {
            Some(expr) => Ok(expr),
            None => session.fail(Error::State),
        }
    }
    fn discard<A: Admission>(&mut self, id: Id, session: &mut Session<'_, A>) -> Step<()> {
        let shallow = match &self.get(id, session)?.kind {
            Kind::Repetition { child, .. } => match &self.get(*child, session)?.kind {
                Kind::Repetition { .. } => false,
                Kind::Concat(children) | Kind::Alternation(children) => children.len() == 0,
                _ => true,
            },
            Kind::Concat(children) | Kind::Alternation(children) => children.len() == 0,
            _ => true,
        };
        if shallow {
            let expr = self.take(id, session)?;
            if let Kind::Repetition { child, .. } = &expr.kind {
                drop(self.take(*child, session)?);
            }
            drop(expr);
            return Ok(());
        }
        // Hir::drop first reserves vec![...] storage, then creates the Empty
        // replacement that remains owned until the original destructor exits.
        let mut pending = Buffer::new();
        pending.reserve_next(session)?;
        let replacement = Expr::empty(session)?;
        pending.push(self.take(id, session)?, session)?;
        while let Some(expr) = pending.pop() {
            match &expr.kind {
                Kind::Repetition { child, .. } => {
                    let child_replacement = Expr::empty(session)?;
                    let child = self.take(*child, session)?;
                    pending.push(child, session)?;
                    drop(child_replacement);
                }
                Kind::Concat(children) | Kind::Alternation(children) => {
                    // The pinned exact-size drain reserves its stack extent
                    // before moving any children out of the concatenation.
                    pending.reserve_more(children.len(), session)?;
                    for &child in children.as_slice() {
                        pending.push(self.take(child, session)?, session)?;
                    }
                }
                _ => {}
            }
            drop(expr);
        }
        drop(replacement);
        Ok(())
    }
    pub(super) fn repetition<A: Admission>(
        &mut self,
        child: Expr,
        mut min: u32,
        mut max: Option<u32>,
        greedy: bool,
        session: &mut Session<'_, A>,
    ) -> Step<Expr> {
        // Pinned translation owns the child before Hir::repetition decides
        // whether it can return that child or an empty expression.
        let child = self.retain(child, session)?;
        if self.get(child, session)?.properties.get().maximum_len == Some(0) {
            min = min.min(1);
            max = Some(max.map_or(1, |max| max.min(1)));
        }
        if min == 0 && max == Some(0) {
            let empty = Expr::empty(session)?;
            self.discard(child, session)?;
            return Ok(empty);
        }
        if min == 1 && max == Some(1) {
            return self.take(child, session);
        }
        let properties =
            Properties::repetition(&self.get(child, session)?.properties, min, max, session)?;
        Ok(Expr {
            kind: Kind::Repetition {
                min,
                max,
                greedy,
                child,
            },
            properties,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Literal, Look};
    use super::*;
    use crate::input::membership::manifest::{reference::Account, Stop};
    #[test]
    fn repetition_normalizes_zero_width_children_and_exact_counts() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut arena = Arena::new();
        let anchor = Expr::look(Look::Start, &mut session).unwrap();
        let identity = arena
            .repetition(anchor, 1, None, true, &mut session)
            .unwrap();
        assert!(matches!(identity.kind, Kind::Look(Look::Start)));
        assert!(arena.0.as_slice()[0].is_none());
        let anchor = Expr::look(Look::End, &mut session).unwrap();
        let optional = arena
            .repetition(anchor, 0, None, false, &mut session)
            .unwrap();
        assert!(matches!(
            optional.kind,
            Kind::Repetition {
                min: 0,
                max: Some(1),
                greedy: false,
                ..
            }
        ));
        assert_eq!(optional.properties.get().look_set_suffix, 0);
        let fail = Expr::fail(&mut session).unwrap();
        let empty = arena
            .repetition(fail, 0, Some(0), true, &mut session)
            .unwrap();
        assert!(matches!(empty.kind, Kind::Empty));
    }
    #[test]
    fn deeply_nested_repetition_has_flat_cleanup() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut arena = Arena::new();
        let mut expr =
            Expr::literal(Literal::copy(b"a", &mut session).unwrap(), &mut session).unwrap();
        for _ in 0..10_000 {
            expr = arena.repetition(expr, 1, None, true, &mut session).unwrap();
        }
        assert_eq!(arena.0.len(), 10_000);
        assert_eq!(expr.properties.get().minimum_len, Some(1));
        drop(expr);
        drop(arena);
    }
    #[test]
    fn zero_count_releases_every_nested_descendant() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut arena = Arena::new();
        let mut expr =
            Expr::literal(Literal::copy(b"a", &mut session).unwrap(), &mut session).unwrap();
        for _ in 0..100 {
            expr = arena.repetition(expr, 1, None, true, &mut session).unwrap();
        }
        let empty = arena
            .repetition(expr, 0, Some(0), true, &mut session)
            .unwrap();
        assert!(matches!(empty.kind, Kind::Empty));
        assert!(arena.0.as_slice().iter().all(Option::is_none));
    }
    struct Count {
        calls: usize,
        refuse: usize,
    }
    #[test]
    fn structural_disposal_preserves_replacement_allocation_order() {
        struct Allocations(Vec<usize>);
        impl Admission for Allocations {
            type Error = ();
            fn admit(&mut self, request: rawr_toml::document::Request) -> Result<(), ()> {
                if let rawr_toml::document::Request::Allocation { bytes, .. } = request {
                    self.0.push(bytes);
                }
                Ok(())
            }
        }
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut arena = Arena::new();
        let leaf = Expr::literal(Literal::copy(b"a", &mut session).unwrap(), &mut session).unwrap();
        let property_bytes = std::mem::size_of_val(leaf.properties.get());
        let first = arena.repetition(leaf, 1, None, true, &mut session).unwrap();
        let second = arena
            .repetition(first, 1, None, true, &mut session)
            .unwrap();
        let mut allocations = Allocations(Vec::new());
        arena
            .repetition(
                second,
                0,
                Some(0),
                true,
                &mut Session::new(&mut allocations),
            )
            .unwrap();
        assert_eq!(
            allocations.0,
            [
                property_bytes,                  // result Empty
                4 * std::mem::size_of::<Expr>(), // vec![...] before its element
                property_bytes,                  // outer replacement
                property_bytes,                  // first child replacement
                property_bytes,                  // second child replacement
            ]
        );
        assert!(arena.0.as_slice().iter().all(Option::is_none));
        let leaf = Expr::look(Look::Start, &mut session).unwrap();
        let id = arena.retain(leaf, &mut session).unwrap();
        let mut allocations = Allocations(Vec::new());
        arena
            .discard(id, &mut Session::new(&mut allocations))
            .unwrap();
        assert!(allocations.0.is_empty());
    }
    impl Admission for Count {
        type Error = ();
        fn admit(&mut self, _: rawr_toml::document::Request) -> Result<(), ()> {
            let call = self.calls;
            self.calls += 1;
            if call == self.refuse {
                Err(())
            } else {
                Ok(())
            }
        }
    }
    #[test]
    fn repetition_refusal_precedes_retention_and_property_work() {
        for (min, max) in [(0, Some(0)), (1, Some(1)), (0, None)] {
            let mut count = Count {
                calls: 0,
                refuse: usize::MAX,
            };
            let child = Expr::look(Look::Start, &mut Session::new(&mut Account)).unwrap();
            Arena::new()
                .repetition(child, min, max, true, &mut Session::new(&mut count))
                .unwrap();
            for refuse in 0..count.calls {
                let mut count = Count { calls: 0, refuse };
                let child = Expr::look(Look::Start, &mut Session::new(&mut Account)).unwrap();
                let mut session = Session::new(&mut count);
                assert!(matches!(
                    Arena::new().repetition(child, min, max, true, &mut session),
                    Err(Stop::Refused)
                ));
                assert!(matches!(session.check(), Err(Stop::Refused)));
                assert_eq!(count.calls, refuse + 1);
            }
        }
    }
}
