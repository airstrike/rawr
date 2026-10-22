//! Read-only borrowed traversal of validated flat HIR ownership.
use super::super::{bytes, unicode};
use super::{
    arena::{Arena, Id},
    Expr, Kind as HirKind, Look,
};
use crate::input::membership::manifest::{session::Session, Step};
use rawr_toml::document::Admission;

#[derive(Clone, Copy)]
pub(in super::super::super) struct View<'a> {
    arena: &'a Arena,
    expr: &'a Expr,
}
pub(in super::super::super) enum Kind<'a> {
    Empty,
    Literal(&'a [u8]),
    Bytes(&'a bytes::Class),
    Unicode(&'a unicode::Class),
    Anchor {
        end: bool,
    },
    Concat(Children<'a>),
    Alternation(Children<'a>),
    Repetition {
        min: u32,
        max: Option<u32>,
        greedy: bool,
        child: View<'a>,
    },
}
#[derive(Clone)]
pub(in super::super::super) struct Children<'a> {
    arena: &'a Arena,
    ids: std::slice::Iter<'a, Id>,
}
impl<'a> View<'a> {
    pub(in super::super::super) fn anchored<A: Admission>(
        &self,
        reverse: bool,
        session: &mut Session<'_, A>,
    ) -> Step<bool> {
        session.items(1)?;
        let properties = self.expr.properties.get();
        Ok(if reverse {
            properties.look_set_suffix & 2 != 0
        } else {
            properties.look_set_prefix & 1 != 0
        })
    }
    pub(in super::super::super) fn is_literal<A: Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<bool> {
        session.items(1)?;
        Ok(matches!(self.expr.kind, HirKind::Literal(_)))
    }
    pub(super) fn new(arena: &'a Arena, expr: &'a Expr) -> Self {
        Self { arena, expr }
    }
    pub(in super::super::super) fn minimum_len<A: Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<Option<usize>> {
        session.items(1)?;
        Ok(self.expr.properties.get().minimum_len)
    }
    pub(in super::super::super) fn kind<A: Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<Kind<'a>> {
        session.items(1)?;
        Ok(match &self.expr.kind {
            HirKind::Empty => Kind::Empty,
            HirKind::Literal(literal) => Kind::Literal(literal.bytes()),
            HirKind::Bytes(class) => Kind::Bytes(class),
            HirKind::Unicode(class) => Kind::Unicode(class),
            HirKind::Look(look) => Kind::Anchor {
                end: matches!(look, Look::End),
            },
            HirKind::Concat(ids) => Kind::Concat(Children {
                arena: self.arena,
                ids: ids.as_slice().iter(),
            }),
            HirKind::Alternation(ids) => Kind::Alternation(Children {
                arena: self.arena,
                ids: ids.as_slice().iter(),
            }),
            HirKind::Repetition {
                min,
                max,
                greedy,
                child,
            } => Kind::Repetition {
                min: *min,
                max: *max,
                greedy: *greedy,
                child: Self::new(self.arena, self.arena.get(*child, session)?),
            },
        })
    }
}
impl<'a> Children<'a> {
    pub(in super::super::super) fn len(&self) -> usize {
        self.ids.len()
    }
    pub(in super::super::super) fn next<A: Admission>(
        &mut self,
        reverse: bool,
        session: &mut Session<'_, A>,
    ) -> Step<Option<View<'a>>> {
        session.items(1)?;
        let id = if reverse {
            self.ids.next_back()
        } else {
            self.ids.next()
        };
        match id {
            None => Ok(None),
            Some(id) => Ok(Some(View::new(self.arena, self.arena.get(*id, session)?))),
        }
    }
}
