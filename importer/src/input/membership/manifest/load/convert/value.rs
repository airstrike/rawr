//! Flat owned copy of an already interpreted TOML value. Children use indices
//! so nested values and partially copied trees have bounded teardown depth.
use crate::input::membership::manifest::{
    load::normalize::target, session::Session, storage, value, Error, Step,
};
use rawr_toml::document;

pub(in super::super) struct Owned {
    nodes: storage::Buffer<Node>,
}
struct Node {
    kind: Kind,
    children: storage::Buffer<(Option<String>, usize)>,
}
enum Kind {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    DateTime(document::Datetime),
    Array,
    Table,
}
struct Frame {
    parent: usize,
    next: Option<usize>,
}

pub(in super::super) fn copy<A: document::Admission>(
    input: &value::Projection<'_>,
    session: &mut Session<'_, A>,
) -> Step<Owned> {
    session.check()?;
    if value::Input::replay(input, 0).mode() != value::Mode::Toml {
        return session.fail(Error::State);
    }
    let mut nodes = storage::Buffer::new();
    let mut frames = storage::Buffer::new();
    enter(input, 0, &mut nodes, &mut frames, session)?;
    while let Some(mut frame) = frames.pop() {
        session.items(1)?;
        if let Some(edge) = frame.next {
            let edge = &input.edges.as_slice()[edge];
            frame.next = edge.next;
            let parent = frame.parent;
            let key = edge
                .key
                .map(|key| target::formatted(format_args!("{}", key.text.as_str()), session))
                .transpose()?;
            session.items(frames.len())?;
            frames.push(frame, session)?;
            let child = enter(input, edge.child, &mut nodes, &mut frames, session)?;
            let children = &mut nodes.as_mut_slice()[parent].children;
            session.items(children.len())?;
            children.push((key, child), session)?;
        }
    }
    Ok(Owned { nodes })
}

fn enter<A: document::Admission>(
    input: &value::Projection<'_>,
    source: usize,
    nodes: &mut storage::Buffer<Node>,
    frames: &mut storage::Buffer<Frame>,
    session: &mut Session<'_, A>,
) -> Step<usize> {
    session.items(1)?;
    let (kind, cursor) = match input.nodes.as_slice()[source].kind {
        value::Kind::Sequence(children) => (Kind::Array, Some(children.first)),
        value::Kind::Map(children) => (Kind::Table, Some(children.first)),
        kind => (
            match kind {
                value::Kind::String(text) => Kind::String(target::formatted(
                    format_args!("{}", text.as_str()),
                    session,
                )?),
                value::Kind::I64(value) => Kind::Integer(value),
                value::Kind::Float(value) => Kind::Float(value),
                value::Kind::Boolean(value) => Kind::Boolean(value),
                value::Kind::DateTime(value) => Kind::DateTime(value),
                _ => return session.fail(Error::State),
            },
            None,
        ),
    };
    session.items(nodes.len())?;
    let index = nodes.push(
        Node {
            kind,
            children: storage::Buffer::new(),
        },
        session,
    )?;
    if let Some(cursor) = cursor {
        session.items(frames.len())?;
        frames.push(
            Frame {
                parent: index,
                next: cursor,
            },
            session,
        )?;
    }
    Ok(index)
}

impl Owned {
    /// The caller has completed the full clone before this type test is reached.
    pub(in super::super) fn strings<A: document::Admission>(
        mut self,
        session: &mut Session<'_, A>,
    ) -> Step<Result<storage::Buffer<String>, ()>> {
        session.check()?;
        if !matches!(self.nodes.as_slice()[0].kind, Kind::Array) {
            drop(self);
            return Ok(Err(()));
        }
        let children = std::mem::replace(
            &mut self.nodes.as_mut_slice()[0].children,
            storage::Buffer::new(),
        );
        let mut result = storage::Buffer::new();
        let mut children = children.into_iter();
        while let Some((_, index)) = children.next() {
            session.items(1)?;
            let kind = std::mem::replace(&mut self.nodes.as_mut_slice()[index].kind, Kind::Array);
            let text = match kind {
                Kind::String(text) => text,
                Kind::DateTime(value) => {
                    let text =
                        crate::input::membership::manifest::datetime::Text::render(value, session)?;
                    target::formatted(format_args!("{}", text.as_str()), session)?
                }
                _ => {
                    drop(result);
                    drop(self);
                    drop(children);
                    return Ok(Err(()));
                }
            };
            session.items(result.len())?;
            result.push(text, session)?;
        }
        Ok(Ok(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::{reference, walk};
    #[test]
    fn clone_owns_strings_and_defers_datetime_deserialization() {
        let mut account = reference::Account;
        let owned = {
            let document = document::parse("v=['cfg(x)',1979-05-27]", 100, &mut account).unwrap();
            let mut session = Session::new(&mut account);
            let input = walk::run(
                value::Input::raw(document.table().get("v").unwrap()),
                value::Mode::Toml,
                &mut session,
            )
            .unwrap();
            let owned = copy(&input, &mut session).unwrap();
            assert!(matches!(owned.nodes.as_slice()[2].kind, Kind::DateTime(_)));
            owned
        };
        let mut session = Session::new(&mut account);
        assert_eq!(
            owned.strings(&mut session).unwrap().unwrap().as_slice(),
            ["cfg(x)", "1979-05-27"]
        );
    }
    #[test]
    fn nested_invalid_list_is_fully_cloned_before_type_rejection() {
        let mut account = reference::Account;
        let document =
            document::parse("v=['first',{a=['deep',true,42,1.5]}]", 100, &mut account).unwrap();
        let mut session = Session::new(&mut account);
        let input = walk::run(
            value::Input::raw(document.table().get("v").unwrap()),
            value::Mode::Toml,
            &mut session,
        )
        .unwrap();
        let owned = copy(&input, &mut session).unwrap();
        assert!(owned
            .nodes
            .as_slice()
            .iter()
            .any(|node| matches!(&node.kind, Kind::String(text) if text == "deep")));
        assert!(owned
            .nodes
            .as_slice()
            .iter()
            .any(|node| matches!(node.kind, Kind::Integer(42))));
        assert!(owned
            .nodes
            .as_slice()
            .iter()
            .any(|node| matches!(node.kind, Kind::Boolean(true))));
        assert!(owned
            .nodes
            .as_slice()
            .iter()
            .any(|node| matches!(node.kind, Kind::Float(value) if value == 1.5)));
        assert!(owned.strings(&mut session).unwrap().is_err());
    }
}
