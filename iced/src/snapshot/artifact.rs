//! One identity-, input-, and theme-proven image artifact.

use rawr::{application, preview, type_def};
use std::path::PathBuf;
use std::sync::Arc;

use crate::widget;

use super::{headless, image, Error};

pub(super) struct Artifact {
    identity: String,
    fingerprint: String,
    theme: Theme,
    relative: PathBuf,
    logical: preview::LogicalSize,
    image: image::Image,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum Theme {
    Native,
    Explicit { index: usize, name: String },
}

impl Artifact {
    pub(super) fn render(
        catalog: &crate::preview::catalog::Authority,
        definition: &preview::Definition,
        theme: Theme,
        renderer: &mut headless::Renderer,
    ) -> Result<Self, Error> {
        let identity = definition.key().to_string();
        let fingerprint = definition
            .digest()
            .map_err(|source| Error::Digest {
                identity: identity.clone(),
                source,
            })?
            .to_string();
        let identity_digest =
            preview::digest::subject(definition.key().subject()).map_err(|source| {
                Error::Digest {
                    identity: identity.clone(),
                    source,
                }
            })?;
        let state = catalog
            .seed(definition.key())
            .map_err(|source| Error::Runtime {
                identity: identity.clone(),
                stage: "seed",
                source,
            })?;
        let materialized = match &theme {
            Theme::Native => crate::theme::Value::default(),
            Theme::Explicit { index, name } => catalog
                .theme(definition.key(), *index, state.as_ref())
                .map_err(|source| Error::Runtime {
                    identity: identity.clone(),
                    stage: "theme",
                    source,
                })
                .and_then(|value| {
                    crate::theme::Value::from_dynamic(value).map_err(|source| Error::Theme {
                        identity: identity.clone(),
                        theme: name.clone(),
                        source,
                    })
                })?,
        };
        let value = catalog
            .subject(definition.key(), state.as_ref())
            .map_err(|source| Error::Runtime {
                identity: identity.clone(),
                stage: "subject",
                source,
            })?;
        let display = widget::Value::from_dynamic(value).map_err(|source| Error::Widget {
            identity: identity.clone(),
            source,
        })?;
        let instance = application::Instance::new();
        let lifecycle = Arc::new(widget::diagnostic::Sink::new(
            instance,
            catalog.generation(),
        ));
        let context = widget::Context::preview(
            catalog,
            definition.key(),
            instance,
            &lifecycle,
            &materialized,
        );
        let frame = renderer.framing();
        let logical = definition.size();
        let declared = iced::Size::new(logical.width() as f32, logical.height() as f32);
        // What its widgets publish goes through the preview's update, when
        // it has a State.
        let mut state = state;
        let mut display = display;
        let mut apply = |published: Vec<crate::runtime::Message>| {
            let Some(current) = state.as_ref().filter(|_| !published.is_empty()) else {
                return display.clone();
            };
            let mut current = current.clone();
            for message in published {
                if let Ok(update) = catalog.update(definition.key(), &current, message.into_payload())
                    && let Ok((next, _)) = update.try_into_parts(Ok::<_, ()>)
                {
                    current = next;
                }
            }
            if let Ok(value) = catalog.subject(definition.key(), Some(&current))
                && let Ok(value) = widget::Value::from_dynamic(value)
            {
                display = value;
            }
            state = Some(current);
            display.clone()
        };
        let size = renderer.size(
            &mut |published| frame(apply(published).lower(&context)),
            declared,
        );
        let image = renderer.render(
            &mut |published| {
                widget::diagnostic::overlay(frame(apply(published).lower(&context)), lifecycle.clone())
            },
            materialized.native(),
            size,
        )?;
        Ok(Self {
            identity,
            fingerprint,
            relative: relative(catalog.package(), definition, &theme, identity_digest),
            logical: definition.size(),
            theme,
            image,
        })
    }

    pub(super) fn identity(&self) -> &str {
        &self.identity
    }

    pub(super) fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    pub(super) const fn theme(&self) -> &Theme {
        &self.theme
    }

    pub(super) fn relative(&self) -> &std::path::Path {
        &self.relative
    }

    pub(super) const fn logical(&self) -> preview::LogicalSize {
        self.logical
    }

    pub(super) const fn image(&self) -> &image::Image {
        &self.image
    }
}

impl Theme {
    pub(super) fn kind(&self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Explicit { .. } => "explicit",
        }
    }

    pub(super) fn name(&self) -> &str {
        match self {
            Self::Native => "iced::Theme::Light",
            Self::Explicit { name, .. } => name,
        }
    }

    pub(super) fn label(&self) -> String {
        match self {
            Self::Native => "native".to_owned(),
            Self::Explicit { index, name } => format!("explicit[{index}] `{name}`"),
        }
    }
}

pub(super) fn themes(definition: &preview::Definition) -> Vec<Theme> {
    std::iter::once(Theme::Native)
        .chain(
            definition
                .themes()
                .iter()
                .enumerate()
                .map(|(index, theme)| Theme::Explicit {
                    index,
                    name: theme.name().to_owned(),
                }),
        )
        .collect()
}

fn relative(
    package: &str,
    definition: &preview::Definition,
    theme: &Theme,
    digest: preview::Digest,
) -> PathBuf {
    let subject = definition.key().subject();
    let owner = match subject.owner() {
        Some(type_def::Identity::Script(owner)) => Some(owner.name()),
        Some(type_def::Identity::Host(_)) | None => None,
    };
    path(
        Coordinates {
            root: package,
            package: subject.package(),
            modules: subject.modules().iter().map(|module| module.as_str()),
            owner,
            subject: subject.name(),
            variant: definition.key().variant(),
        },
        theme,
        &suffix(digest),
    )
}

struct Coordinates<'a, Modules> {
    root: &'a str,
    package: &'a str,
    modules: Modules,
    owner: Option<&'a str>,
    subject: &'a str,
    variant: &'a str,
}

fn path<'a>(
    coordinates: Coordinates<'a, impl IntoIterator<Item = &'a str>>,
    theme: &Theme,
    suffix: &str,
) -> PathBuf {
    let Coordinates {
        root,
        package,
        modules,
        owner,
        subject,
        variant,
    } = coordinates;
    let mut path = PathBuf::new();
    if package != root {
        path.push("packages");
        path.push(human(package));
    }
    path.extend(modules.into_iter().map(human));
    path.push(format!("{}-{suffix}", human(owner.unwrap_or(subject))));
    if owner.is_some() {
        path.push(human(subject));
    }
    path.push(human(variant));
    path.push(match theme {
        Theme::Native => "native.png".to_owned(),
        Theme::Explicit { name, .. } => format!("{}.png", human(name)),
    });
    path
}

fn suffix(digest: preview::Digest) -> String {
    let bytes = digest.bytes();
    format!(
        "{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3]
    )
}

fn human(value: &str) -> String {
    const MAX: usize = 48;

    let mut normalized = String::with_capacity(value.len().min(MAX));
    let mut separator = false;
    for character in value.chars() {
        if normalized.len() >= MAX {
            break;
        }
        if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
            normalized.push(character);
            separator = false;
        } else if !normalized.is_empty() && !separator {
            normalized.push('-');
            separator = true;
        }
    }
    while normalized.ends_with('-') {
        normalized.pop();
    }
    if normalized.is_empty() {
        normalized.push_str("preview");
    }
    let reserved = normalized.to_ascii_uppercase();
    if matches!(reserved.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || reserved
            .strip_prefix("COM")
            .or_else(|| reserved.strip_prefix("LPT"))
            .is_some_and(|number| {
                matches!(number, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            })
    {
        normalized.insert(0, '_');
    }
    normalized
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::Theme;

    #[test]
    fn readable_paths_distinguish_root_and_dependency_packages() {
        let root = super::path(
            super::Coordinates {
                root: "counter",
                package: "counter",
                modules: std::iter::empty(),
                owner: Some("Counter"),
                subject: "view",
                variant: "default",
            },
            &Theme::Native,
            "a81f42c0",
        );
        assert_eq!(root, Path::new("Counter-a81f42c0/view/default/native.png"));
        let sibling = super::path(
            super::Coordinates {
                root: "counter",
                package: "counter",
                modules: std::iter::empty(),
                owner: Some("Counter"),
                subject: "view",
                variant: "forty-two",
            },
            &Theme::Native,
            "a81f42c0",
        );
        assert_eq!(
            sibling,
            Path::new("Counter-a81f42c0/view/forty-two/native.png")
        );

        let dependency = super::path(
            super::Coordinates {
                root: "app",
                package: "controls",
                modules: std::iter::empty(),
                owner: Some("Button"),
                subject: "view",
                variant: "default",
            },
            &Theme::Explicit {
                index: 0,
                name: "dark".to_owned(),
            },
            "b72c195e",
        );
        assert_eq!(
            dependency,
            Path::new("packages/controls/Button-b72c195e/view/default/dark.png")
        );
    }

    #[test]
    fn human_components_are_safe_and_independent_from_the_digest() {
        assert_eq!(super::human("Fancy / Counter"), "Fancy-Counter");
        assert_eq!(super::human("雪"), "preview");
        assert_eq!(super::human("CON"), "_CON");
    }
}
