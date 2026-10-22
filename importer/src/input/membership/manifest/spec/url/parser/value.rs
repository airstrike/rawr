//! Session-checked URL borrows and the reached removal-only mutations.

use std::cmp::Ordering;

use rawr_toml::document;

use super::Url;
use crate::input::membership::manifest::{session, Step};

impl PartialEq for Url {
    fn eq(&self, other: &Self) -> bool {
        self.output.as_bytes() == other.output.as_bytes()
    }
}

impl Eq for Url {}

impl PartialOrd for Url {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Url {
    fn cmp(&self, other: &Self) -> Ordering {
        self.output.as_bytes().cmp(other.output.as_bytes())
    }
}

impl Url {
    pub(in crate::input::membership::manifest) fn copy<A: document::Admission>(
        &self,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        let mut output = super::output::Output::new();
        output.append(self.as_str(session)?, session)?;
        session.items(1)?;
        Ok(Self {
            output,
            scheme_end: self.scheme_end,
            path_start: self.path_start,
            query_start: self.query_start,
            fragment_start: self.fragment_start,
        })
    }

    pub(in crate::input::membership::manifest) fn password<A: document::Admission>(
        &self,
        session: &mut session::Session<'_, A>,
    ) -> Step<Option<&str>> {
        let text = self.as_str(session)?;
        let authority = &text[self.scheme_end as usize + 1..self.path_start as usize];
        let Some(authority) = authority.strip_prefix("//") else {
            return Ok(None);
        };
        let Some((userinfo, _)) = authority.rsplit_once('@') else {
            return Ok(None);
        };
        Ok(userinfo.split_once(':').map(|(_, password)| password))
    }
    pub(in crate::input::membership::manifest) fn as_str<A: document::Admission>(
        &self,
        session: &mut session::Session<'_, A>,
    ) -> Step<&str> {
        self.output.as_str(session)
    }

    pub(in super::super::super) fn scheme<A: document::Admission>(
        &self,
        session: &mut session::Session<'_, A>,
    ) -> Step<&str> {
        Ok(&self.as_str(session)?[..self.scheme_end as usize])
    }

    pub(in super::super::super) fn query<A: document::Admission>(
        &self,
        session: &mut session::Session<'_, A>,
    ) -> Step<Option<&str>> {
        let text = self.as_str(session)?;
        Ok(self.query_start.map(|start| {
            &text[start as usize + 1..self.fragment_start.map_or(text.len(), |i| i as usize)]
        }))
    }

    pub(in crate::input::membership::manifest) fn fragment<A: document::Admission>(
        &self,
        session: &mut session::Session<'_, A>,
    ) -> Step<Option<&str>> {
        let text = self.as_str(session)?;
        Ok(self.fragment_start.map(|start| &text[start as usize + 1..]))
    }

    pub(in super::super::super) fn last_path_segment<A: document::Admission>(
        &self,
        session: &mut session::Session<'_, A>,
    ) -> Step<Option<&str>> {
        let text = self.as_str(session)?;
        let end = self
            .query_start
            .or(self.fragment_start)
            .map_or(text.len(), |i| i as usize);
        let path = &text[self.path_start as usize..end];
        Ok(path
            .strip_prefix('/')
            .and_then(|path| path.rsplit('/').next()))
    }

    pub(in super::super::super) fn remove_query<A: document::Admission>(
        &mut self,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        session.check()?;
        if let Some(start) = self.query_start {
            let end = self
                .fragment_start
                .map_or(self.output.len(), |i| i as usize);
            self.output.remove(start as usize..end, session)?;
            self.query_start = None;
            if self.fragment_start.is_some() {
                self.fragment_start = Some(start);
            }
        }
        self.strip_spaces(session)
    }

    pub(in super::super::super) fn remove_fragment<A: document::Admission>(
        &mut self,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        session.check()?;
        if let Some(start) = self.fragment_start {
            self.output.truncate(start as usize, session)?;
            self.fragment_start = None;
        }
        self.strip_spaces(session)
    }

    fn strip_spaces<A: document::Admission>(
        &mut self,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        session.check()?;
        if self.query_start.is_none()
            && self.fragment_start.is_none()
            && self.output.as_bytes().get(self.scheme_end as usize + 1) != Some(&b'/')
        {
            let count = self
                .output
                .as_bytes()
                .iter()
                .rev()
                .take_while(|&&b| b == b' ')
                .count();
            self.output.truncate(self.output.len() - count, session)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod password_tests {
    use super::*;
    #[test]
    fn serialized_password_matches_pinned_url_getter() {
        for text in [
            "https://user:secret@example.com/",
            "https://user:@example.com/",
            "https://user@example.com/",
            "https://u%3Ax:p%40ss@example.com/",
            "https://u:p@x@example.com/",
            "https://u:p@[::1]:1234/",
            "custom:user:pass@example.com",
            "file:///a@b:c",
            "https://:p@example.com/",
        ] {
            let reference = ::url::Url::parse(text).unwrap();
            let mut account = crate::input::membership::manifest::reference::Account;
            let mut session = session::Session::new(&mut account);
            let parsed = Url::parse(text, &mut session).unwrap().ok().unwrap();
            assert_eq!(
                parsed.password(&mut session).unwrap(),
                reference.password(),
                "{text}"
            );
            session.finish(Ok(())).unwrap();
        }
    }
}
