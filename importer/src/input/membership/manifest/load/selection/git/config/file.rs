// Copyright the gitoxide contributors. MIT OR Apache-2.0.
//! Source construction, duplicate lookup ownership, ordered append and access.

use super::*;

impl File {
    pub(super) fn empty<A: Admission>(meta: Metadata, session: &mut Session<'_, A>) -> Step<Self> {
        Self::with_meta(shared(meta, session)?, session)
    }
    fn with_meta<A: Admission>(meta: Arc<Metadata>, session: &mut Session<'_, A>) -> Step<Self> {
        session.items(1)?;
        Ok(Self {
            front: Front::new(),
            post: table::Map::new(),
            lookup: table::Map::new(),
            sections: table::Map::new(),
            next: 0,
            order: storage::Order::default(),
            meta,
        })
    }
    pub(super) fn parse<A: Admission>(
        input: &[u8],
        meta: Metadata,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Self, parse::Failure>> {
        let parsed = match parse::parse(input, session)? {
            Ok(parsed) => parsed,
            Err(error) => return Ok(Err(error)),
        };
        let meta = shared(meta, session)?;
        session.items(1)?;
        let mut file = Self::with_meta(Arc::clone(&meta), session)?;
        file.front = parsed.front;
        file.sections.reserve(parsed.sections.len(), session)?;
        file.order.reserve(parsed.sections.len(), session)?;
        for section in parsed.sections.into_iter() {
            session.items(1)?;
            file.push(
                Section {
                    header: section.header,
                    body: section.events,
                    meta: Arc::clone(&meta),
                    id: Id(usize::MAX),
                },
                session,
            )?;
        }
        // Includes::no_follow reaches the zero-depth return before allocation.
        session.items(1)?;
        Ok(Ok(file))
    }
    fn push<A: Admission>(
        &mut self,
        mut section: Section,
        session: &mut Session<'_, A>,
    ) -> Step<Id> {
        let id = Id(self.next);
        section.id = id;
        self.sections.insert(id, section, session)?;
        let Some(section) = self.sections.get(&id, session)? else {
            return session.fail(Error::State);
        };
        let key = Name(Cow::Owned(own(&section.header.name.0, session)?));
        let lookup = self.lookup.entry(key, session)?;
        let subsection = section
            .header
            .subsection
            .as_ref()
            .map(|s| own(s, session).map(Cow::Owned))
            .transpose()?;
        let mut found = false;
        if let Some(subsection) = subsection {
            for node in lookup.as_mut_slice() {
                session.items(1)?;
                if let Lookup::NonTerminal(map) = node {
                    found = true;
                    let copy = Cow::Owned(own(&subsection, session)?);
                    map.entry(copy, session)?.push(id, session)?;
                    break;
                }
            }
            if !found {
                let mut map = table::Map::new();
                let mut ids = storage::Vector::with_capacity(1, session)?;
                ids.push(id, session)?;
                map.insert(subsection, ids, session)?;
                lookup.push(Lookup::NonTerminal(map), session)?;
            }
        } else {
            for node in lookup.as_mut_slice() {
                session.items(1)?;
                if let Lookup::Terminal(ids) = node {
                    found = true;
                    ids.push(id, session)?;
                    break;
                }
            }
            if !found {
                let mut ids = storage::Vector::with_capacity(1, session)?;
                ids.push(id, session)?;
                lookup.push(Lookup::Terminal(ids), session)?;
            }
        }
        self.order.push(id, session)?;
        let Some(next) = self.next.checked_add(1) else {
            return session.fail(Error::Overflow);
        };
        self.next = next;
        Ok(id)
    }
    pub(super) fn append<A: Admission>(
        &mut self,
        mut other: Self,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        let newline = self.newline(session)?;
        let mut nl = smallvec::SmallVec::<[u8; 2]>::new();
        storage::small_reserve(&mut nl, newline.len(), session)?;
        session.items(newline.len())?;
        nl.extend_from_slice(newline);
        let last = self.next.checked_sub(1).map(Id);
        let order = std::mem::take(&mut other.order);
        for id in order.into_iter() {
            session.items(1)?;
            let Some(section) = other.sections.remove(&id, session)? else {
                return session.fail(Error::State);
            };
            let new_id = self.push(section, session)?;
            if let Some(post) = other.post.remove(&id, session)? {
                self.post.insert(new_id, post, session)?;
            }
        }
        if other.front.is_empty() {
            return Ok(());
        }
        let destination = match last {
            Some(id) => self.post.entry(id, session)?,
            None => &mut self.front,
        };
        let mut ends = destination.is_empty();
        for event in destination.iter().rev() {
            session.items(event.bytes().len())?;
            if !event.bytes().iter().all(u8::is_ascii_whitespace) {
                break;
            }
            session.items(event.bytes().len())?;
            if event
                .bytes()
                .windows(nl.len())
                .any(|bytes| bytes == nl.as_slice())
            {
                ends = true;
                break;
            }
        }
        let starts = if ends {
            true
        } else {
            session.items(nl.len())?;
            other
                .front
                .first()
                .is_none_or(|e| e.bytes().starts_with(&nl))
        };
        if !ends && !starts {
            let newline = own(&nl, session)?;
            storage::small_push(destination, Event::Newline(Cow::Owned(newline)), session)?;
        }
        storage::small_reserve(destination, other.front.len(), session)?;
        session.items(other.front.len())?;
        destination.extend(other.front);
        Ok(())
    }
    fn newline<A: Admission>(&self, session: &mut Session<'_, A>) -> Step<&[u8]> {
        for event in self.front.iter() {
            session.items(1)?;
            if let Some(nl) = event.newline(session)? {
                return Ok(nl);
            }
        }
        for id in self.order.iter() {
            session.items(1)?;
            let Some(section) = self.sections.get(id, session)? else {
                return session.fail(Error::State);
            };
            for event in section.body.as_slice() {
                session.items(1)?;
                if let Some(nl) = event.newline(session)? {
                    return Ok(nl);
                }
            }
        }
        Ok(b"\n")
    }
    fn ids<A: Admission>(
        &self,
        section: &'static [u8],
        sub: Option<&'static [u8]>,
        session: &mut Session<'_, A>,
    ) -> Step<Option<&[Id]>> {
        let Some(lookup) = self.lookup.get(&Name(Cow::Borrowed(section)), session)? else {
            return Ok(None);
        };
        for node in lookup.as_slice() {
            session.items(1)?;
            match (sub, node) {
                (None, Lookup::Terminal(values)) => return Ok(Some(values.as_slice())),
                (Some(sub), Lookup::NonTerminal(map)) => {
                    return Ok(map.get(&Cow::Borrowed(sub), session)?.map(|v| v.as_slice()))
                }
                _ => {}
            }
        }
        Ok(None)
    }
    fn implicit<'a, A: Admission>(
        &'a self,
        section: &'static [u8],
        sub: Option<&'static [u8]>,
        key: &[u8],
        session: &mut Session<'_, A>,
    ) -> Step<Option<Option<Cow<'a, [u8]>>>> {
        let Some(ids) = self.ids(section, sub, session)? else {
            return Ok(None);
        };
        for id in ids.iter().rev() {
            session.items(1)?;
            let Some(section) = self.sections.get(id, session)? else {
                return session.fail(Error::State);
            };
            if let Some(value) = section.value(key, session)? {
                return Ok(Some(value));
            }
        }
        Ok(None)
    }
    pub(super) fn string<'a, A: Admission>(
        &'a self,
        section: &'static [u8],
        sub: Option<&'static [u8]>,
        key: &[u8],
        session: &mut Session<'_, A>,
    ) -> Step<Option<Cow<'a, [u8]>>> {
        // Raw string lookup skips a newer implicit key and searches older
        // sections, unlike Boolean's successful implicit-true branch.
        let Some(ids) = self.ids(section, sub, session)? else {
            return Ok(None);
        };
        for id in ids.iter().rev() {
            session.items(1)?;
            let Some(section) = self.sections.get(id, session)? else {
                return session.fail(Error::State);
            };
            if let Some(Some(value)) = section.value(key, session)? {
                return Ok(Some(value));
            }
        }
        Ok(None)
    }
    pub(super) fn boolean<A: Admission>(
        &self,
        section: &'static [u8],
        sub: Option<&'static [u8]>,
        key: &[u8],
        session: &mut Session<'_, A>,
    ) -> Step<Option<Result<bool, value::Failure>>> {
        match self.implicit(section, sub, key, session)? {
            None => Ok(None),
            Some(None) => Ok(Some(Ok(true))),
            Some(Some(value)) => value::boolean(&value, session).map(Some),
        }
    }
    pub(super) fn integer<A: Admission>(
        &self,
        section: &'static [u8],
        key: &[u8],
        session: &mut Session<'_, A>,
    ) -> Step<Option<Result<i64, value::Failure>>> {
        match self.string(section, None, key, session)? {
            None => Ok(None),
            Some(value) => value::integer(value, session).map(Some),
        }
    }
}

impl Event<'_> {
    fn bytes(&self) -> &[u8] {
        match self {
            Self::Comment { text, .. } => text,
            Self::Header(h) => &h.name.0,
            Self::Name(n) => &n.0,
            Self::Separator => b"=",
            Self::Value(v)
            | Self::Done(v)
            | Self::NotDone(v)
            | Self::Newline(v)
            | Self::Whitespace(v) => v,
        }
    }
    fn newline<A: Admission>(&self, session: &mut Session<'_, A>) -> Step<Option<&[u8]>> {
        match self {
            Self::Newline(v) => {
                session.items(v.len())?;
                Ok(Some(if v.contains(&b'\r') { b"\r\n" } else { b"\n" }))
            }
            _ => Ok(None),
        }
    }
}
impl Section {
    fn value<'a, A: Admission>(
        &'a self,
        key: &[u8],
        session: &mut Session<'_, A>,
    ) -> Step<Option<Option<Cow<'a, [u8]>>>> {
        let mut start = 0;
        let mut end = 0;
        let mut found = None;
        for (index, event) in self.body.as_slice().iter().enumerate().rev() {
            session.items(1)?;
            match event {
                Event::Name(name) => {
                    session.items(key.len().max(name.0.len()))?;
                    if name.0.eq_ignore_ascii_case(key) {
                        found = Some(index);
                        break;
                    }
                    start = 0;
                    end = 0;
                }
                Event::Value(_) => {
                    start = index;
                    end = index;
                }
                Event::NotDone(_) | Event::Done(_) => {
                    if end == 0 {
                        end = index;
                    } else {
                        start = index;
                    }
                }
                _ => {}
            }
        }
        let Some(key_start) = found else {
            return Ok(None);
        };
        if start == key_start + 1 {
            return Ok(Some(None));
        }
        let mut concatenated = storage::Vector::new();
        for event in &self.body.as_slice()[start..end + 1] {
            session.items(1)?;
            match event {
                Event::Value(bytes) => {
                    return value::normalize(Cow::Borrowed(bytes), session).map(|v| Some(Some(v)))
                }
                Event::NotDone(bytes) => concatenated.extend_copy(bytes, session)?,
                Event::Done(bytes) => {
                    concatenated.extend_copy(bytes, session)?;
                    return value::normalize(Cow::Owned(concatenated.into_values()), session)
                        .map(|v| Some(Some(v)));
                }
                _ => {}
            }
        }
        Ok(None)
    }
}
