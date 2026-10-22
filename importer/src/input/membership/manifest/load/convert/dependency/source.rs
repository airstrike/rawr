//! Pinned to_dependency_source_id, reached after normalized dependency checks.
use super::{inherit, primitive, target, text, Error, Field, Session, Step, Warnings};
use crate::input::membership::manifest::{
    load::{
        config,
        convert::{
            source::{self, Git, Kind, Origin, Selector},
            url,
        },
        path,
    },
    scalar,
};
use crate::{cargo, input::membership::host};
use std::path::Path;

pub(super) fn select<'a, 'd: 'a, 'store, 'image: 'store>(
    name: &str,
    get: impl Fn(Field) -> Option<inherit::Value<'a, 'd>>,
    manifest: &Path,
    parent: source::Id<'store>,
    sources: &mut source::Owner<'store>,
    config: &mut config::Owner<'store, 'image>,
    limits: &cargo::Limits,
    warnings: &mut Warnings,
    session: &mut Session<'_, host::Host<'image, '_>>,
) -> Step<Result<source::Id<'store>, String>> {
    session.check()?;
    let git = text(get(Field::Git), session)?;
    let local = text(get(Field::Path), session)?;
    let registry = match get(Field::Registry) {
        None => None,
        Some(inherit::Value::Original(primitive::Value::Name(name))) => Some(name.as_str()),
        _ => return session.fail(Error::State),
    };
    let index = text(get(Field::RegistryIndex), session)?;
    if git.is_some() && local.is_some() {
        return target::formatted(format_args!("dependency ({name}) specification is ambiguous. Only one of `git` or `path` is allowed."), session).map(Err);
    }
    if registry.is_some() && index.is_some() {
        return target::formatted(format_args!("dependency ({name}) specification is ambiguous. Only one of `registry` or `registry-index` is allowed."), session).map(Err);
    }
    if let Some(git) = git {
        let branch = text(get(Field::Branch), session)?;
        let tag = text(get(Field::Tag), session)?;
        let rev = text(get(Field::Rev), session)?;
        session.items(3)?;
        if usize::from(branch.is_some()) + usize::from(tag.is_some()) + usize::from(rev.is_some())
            > 1
        {
            return target::formatted(format_args!("dependency ({name}) specification is ambiguous. Only one of `branch`, `tag` or `rev` is allowed."), session).map(Err);
        }
        let reference = if let Some(value) = branch {
            Git::Branch(value)
        } else if let Some(value) = tag {
            Git::Tag(value)
        } else if let Some(value) = rev {
            Git::Rev(value)
        } else {
            Git::DefaultBranch
        };
        let reference = Selector::from(reference, session)?;
        let url = match url::parse(git, session)? {
            Ok(url) => url,
            Err(error) => return Ok(Err(error)),
        };
        if let Some(fragment) = url.fragment(session)? {
            let warning = target::formatted(format_args!("URL fragment `#{fragment}` in git URL is ignored for dependency ({name}). If you were trying to specify a specific git revision, use `rev = \"{fragment}\"` in the dependency declaration."), session)?;
            warnings.push_formatted(warning, session)?;
        }
        return sources.git(&url, reference, session);
    }
    if let Some(local) = local {
        let resolved = path::copy(Path::new(local), session)?;
        if !matches!(sources.get(parent, session)?.kind(), Kind::Path) {
            return Ok(Ok(parent));
        }
        let Some(directory) = manifest.parent() else {
            return session.fail(Error::State);
        };
        let joined = path::join(directory, &resolved, session)?;
        let normalized = path::normalize(&joined, session)?;
        let url = match url::path(&normalized, session)? {
            Ok(url) => url,
            Err(error) => return Ok(Err(error)),
        };
        return sources.intern(url, Origin::Path, None, None, session);
    }
    if let Some(registry) = registry {
        return named(registry, sources, config, limits, session);
    }
    if let Some(index) = index {
        let url = match url::parse(index, session)? {
            Ok(url) => url,
            Err(error) => return Ok(Err(error)),
        };
        return sources.registry(&url, None, session);
    }
    config.crates_io(sources, limits, session)
}

pub(super) fn named<'store, 'image: 'store>(
    registry: &str,
    sources: &mut source::Owner<'store>,
    config: &mut config::Owner<'store, 'image>,
    limits: &cargo::Limits,
    session: &mut Session<'_, host::Host<'image, '_>>,
) -> Step<Result<source::Id<'store>, String>> {
    if registry == "crates-io" {
        return config.crates_io(sources, limits, session);
    }
    session.string(registry.len())?;
    let name = match scalar::Name::parse(registry, scalar::Kind::Registry) {
        Ok(name) => name,
        Err(_) => return session.fail(Error::State),
    };
    let url = match config.registry_index(name, limits, session)? {
        Ok(url) => url,
        Err(error) => return Ok(Err(error)),
    };
    sources.registry(&url, Some(registry), session)
}
