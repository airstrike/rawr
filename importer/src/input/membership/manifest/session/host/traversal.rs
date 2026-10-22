//! Traversal operations keep semantic native errors inside the original Session.

use crate::input::membership::manifest::{session::Session, Error, Step};
use crate::input::membership::{host, observed};

impl<'image> Session<'_, host::Host<'image, '_>> {
    pub(in super::super::super) fn walk_begin(
        &mut self,
        root: &[u8],
        is_root: bool,
    ) -> Step<host::traversal::Cursor<'image>> {
        self.check()?;
        match self.admission.traversal(root, is_root) {
            Ok(cursor) => Ok(cursor),
            Err(error) => self.fail(Error::Host(error)),
        }
    }

    pub(in super::super::super) fn walk_open(
        &mut self,
        cursor: &mut host::traversal::Cursor<'image>,
        path: &[u8],
    ) -> Step<host::traversal::Directory> {
        self.check()?;
        match cursor.open(self.admission, path) {
            Ok(directory) => Ok(directory),
            Err(error) => self.fail(Error::Host(error)),
        }
    }

    pub(in super::super::super) fn walk_next(
        &mut self,
        cursor: &mut host::traversal::Cursor<'image>,
        directory: &mut host::traversal::Directory,
    ) -> Step<host::traversal::Read<'image>> {
        self.check()?;
        match cursor.next(self.admission, directory) {
            Ok(read) => Ok(read),
            Err(error) => self.fail(Error::Host(error)),
        }
    }

    pub(in super::super::super) fn walk_close(
        &mut self,
        cursor: &mut host::traversal::Cursor<'image>,
        directory: host::traversal::Directory,
        reason: observed::traversal::Close,
    ) -> Step<()> {
        self.check()?;
        match cursor.close(self.admission, directory, reason) {
            Ok(()) => Ok(()),
            Err(error) => self.fail(Error::Host(error)),
        }
    }

    pub(in super::super::super) fn walk_handle(
        &mut self,
        cursor: &mut host::traversal::Cursor<'image>,
        path: &[u8],
    ) -> Step<Result<host::traversal::Identity, i32>> {
        self.check()?;
        match cursor.handle(self.admission, path) {
            Ok(identity) => Ok(identity),
            Err(error) => self.fail(Error::Host(error)),
        }
    }

    pub(in super::super::super) fn walk_same(
        &mut self,
        cursor: &mut host::traversal::Cursor<'image>,
        left: &host::traversal::Identity,
        right: &host::traversal::Identity,
    ) -> Step<bool> {
        self.check()?;
        match cursor.same(self.admission, left, right) {
            Ok(same) => Ok(same),
            Err(error) => self.fail(Error::Host(error)),
        }
    }

    pub(in super::super::super) fn walk_release(
        &mut self,
        cursor: &mut host::traversal::Cursor<'image>,
        identity: host::traversal::Identity,
    ) -> Step<()> {
        self.check()?;
        match cursor.release(self.admission, identity) {
            Ok(()) => Ok(()),
            Err(error) => self.fail(Error::Host(error)),
        }
    }

    pub(in super::super::super) fn walk_finish(
        &mut self,
        cursor: host::traversal::Cursor<'image>,
    ) -> Step<()> {
        self.check()?;
        match cursor.finish(self.admission) {
            Ok(()) => Ok(()),
            Err(error) => self.fail(Error::Host(error)),
        }
    }

    pub(in super::super::super) fn metadata(
        &mut self,
        path: &[u8],
        follow: bool,
    ) -> Step<Result<observed::Metadata, i32>> {
        self.check()?;
        match self.admission.traversal_metadata(path, follow) {
            Ok(value) => Ok(value),
            Err(error) => self.fail(Error::Host(error)),
        }
    }
}
