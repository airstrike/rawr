//! Legacy migration and selected-entry credential overlays.

use super::*;

enum Copy<'s, 'd> {
    Table {
        destination: usize,
        incoming: table::Iter<'s, 'd>,
    },
    List {
        source: usize,
        destination: usize,
        next: usize,
    },
}

impl<'o, 'd> Tree<'o, 'd> {
    /// This transition precedes lazy ordinary configuration initialization.
    pub(in super::super::super) fn normalize_credentials<A: document::Admission>(
        mut self,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.check()?;
        let definition = self.definition();
        let wrapper_index = self.nodes.len();
        let Some(Kind::Table(root)) = self.nodes.as_mut_slice()[0].kind.as_mut() else {
            return session.fail(Error::State);
        };
        if let Some(token) = root.remove("token", session)? {
            // Entry's occupied branch discards token even for a scalar registry.
            if root.merge("registry", wrapper_index, session)?.is_none() {
                let mut wrapper = table::Table::new();
                wrapper.insert("token", token, session)?;
                let index = self.nodes.push(
                    Node {
                        kind: Some(Kind::Table(wrapper)),
                        definition,
                        span: None,
                    },
                    session,
                )?;
                if index != wrapper_index {
                    return session.fail(Error::State);
                }
            }
        }
        Ok(self)
    }

    /// Ordinary values have initialized successfully before entering this step.
    /// Publish only credential root keys; missing keys remain owner fallbacks.
    pub(in super::super::super) fn merge_credentials<A: document::Admission>(
        mut self,
        ordinary: &Self,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.check()?;
        let Some(Kind::Table(base)) = ordinary.nodes.as_slice()[0].kind.as_ref() else {
            return session.fail(Error::State);
        };
        let Some(Kind::Table(root)) = self.nodes.as_mut_slice()[0].kind.take() else {
            return session.fail(Error::State);
        };
        let mut incoming = root.into_iter(session)?;
        let mut overlay = table::Table::new();
        while let Some((key, value)) = incoming.next(session)? {
            let entry = if let Some(source) = base.get(key, session)? {
                let old = ordinary.copy_subtree(source, &mut self.nodes, session)?;
                // Cargo calls ConfigValue::merge independently for each key.
                // Its key starts empty and does not include this root key.
                self.merge_at(old, value, true, session)?;
                old
            } else {
                value
            };
            overlay.insert(key, entry, session)?;
        }
        self.nodes.as_mut_slice()[0].kind = Some(Kind::Table(overlay));
        Ok(self)
    }

    fn copy_subtree<A: document::Admission>(
        &self,
        source: usize,
        nodes: &mut storage::Buffer<Node<'o, 'd>>,
        session: &mut Session<'_, A>,
    ) -> Step<usize> {
        let mut frames = storage::Buffer::new();
        let root = self.copy_node(source, nodes, &mut frames, session)?;
        while let Some(frame) = frames.pop() {
            session.items(1)?;
            match frame {
                Copy::Table {
                    destination,
                    mut incoming,
                } => {
                    if let Some((key, source)) = incoming.next(session)? {
                        frames.push(
                            Copy::Table {
                                destination,
                                incoming,
                            },
                            session,
                        )?;
                        let child = self.copy_node(source, nodes, &mut frames, session)?;
                        let Some(Kind::Table(table)) =
                            nodes.as_mut_slice()[destination].kind.as_mut()
                        else {
                            return session.fail(Error::State);
                        };
                        table.replace(key, child, session)?;
                    }
                }
                Copy::List {
                    source,
                    destination,
                    next,
                } => {
                    let Some(Kind::List(elements)) = self.nodes.as_slice()[source].kind.as_ref()
                    else {
                        return session.fail(Error::State);
                    };
                    if let Some(element) = elements.as_slice().get(next) {
                        frames.push(
                            Copy::List {
                                source,
                                destination,
                                next: next + 1,
                            },
                            session,
                        )?;
                        let child = self.copy_node(element.node, nodes, &mut frames, session)?;
                        let Some(Kind::List(elements)) =
                            nodes.as_mut_slice()[destination].kind.as_mut()
                        else {
                            return session.fail(Error::State);
                        };
                        elements.push(
                            Element {
                                node: child,
                                order: element.order,
                            },
                            session,
                        )?;
                    }
                }
            }
        }
        Ok(root)
    }

    fn copy_node<'s, A: document::Admission>(
        &'s self,
        source: usize,
        nodes: &mut storage::Buffer<Node<'o, 'd>>,
        frames: &mut storage::Buffer<Copy<'s, 'd>>,
        session: &mut Session<'_, A>,
    ) -> Step<usize> {
        session.items(1)?;
        let node = &self.nodes.as_slice()[source];
        let kind = match node.kind.as_ref() {
            Some(Kind::String(text)) => Kind::String(text),
            Some(Kind::Integer(value)) => Kind::Integer(*value),
            Some(Kind::Boolean(value)) => Kind::Boolean(*value),
            Some(Kind::List(_)) => Kind::List(storage::Buffer::new()),
            Some(Kind::Table(table)) => Kind::Table(table.try_copy(session)?),
            None => return session.fail(Error::State),
        };
        let destination = nodes.push(
            Node {
                kind: Some(kind),
                definition: node.definition,
                span: node.span,
            },
            session,
        )?;
        match node.kind.as_ref() {
            Some(Kind::List(_)) => {
                frames.push(
                    Copy::List {
                        source,
                        destination,
                        next: 0,
                    },
                    session,
                )?;
            }
            Some(Kind::Table(table)) => {
                frames.push(
                    Copy::Table {
                        destination,
                        incoming: table.iter(session)?,
                    },
                    session,
                )?;
            }
            _ => {}
        }
        Ok(destination)
    }
}

#[cfg(test)]
mod tests;
