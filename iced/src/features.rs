//! iced's optional widgets, as the project's iced has them.
//!
//! A project's runner builds rawr-iced against the project's own iced, with
//! only the widget features that iced has (`canvas`, `image`, `qr_code`,
//! `selector`, `svg`), so `cargo build` and the runner share one iced. The
//! `rawr-iced` command is built with all of them and checks a program
//! against the ones its project selects, so both sides declare the same
//! facade.

use std::sync::OnceLock;

/// One optional iced widget feature.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Feature {
    Canvas,
    Image,
    QrCode,
    Selector,
    Svg,
}

impl Feature {
    /// Every optional feature, in name order.
    pub const ALL: [Self; 5] = [
        Self::Canvas,
        Self::Image,
        Self::QrCode,
        Self::Selector,
        Self::Svg,
    ];

    /// The rawr-iced feature name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Canvas => "canvas",
            Self::Image => "image",
            Self::QrCode => "qr_code",
            Self::Selector => "selector",
            Self::Svg => "svg",
        }
    }

    /// The rawr-iced feature an iced feature implies: `image` and
    /// `image-without-codecs` give `image`, the others their own name.
    #[must_use]
    pub fn of_iced(feature: &str) -> Option<Self> {
        match feature {
            "image" | "image-without-codecs" => Some(Self::Image),
            _ => Self::ALL.into_iter().find(|known| known.name() == feature),
        }
    }

    /// Whether this build of rawr-iced has the feature.
    #[must_use]
    pub const fn compiled(self) -> bool {
        match self {
            Self::Canvas => cfg!(feature = "canvas"),
            Self::Image => cfg!(feature = "image"),
            Self::QrCode => cfg!(feature = "qr_code"),
            Self::Selector => cfg!(feature = "selector"),
            Self::Svg => cfg!(feature = "svg"),
        }
    }

    const fn bit(self) -> u8 {
        1 << self as u8
    }
}

static SELECTED: OnceLock<u8> = OnceLock::new();

/// Declare only `features` (the project's) for the rest of this process,
/// so the facade the command checks against is the runner's. Only the
/// first selection counts.
pub fn select(features: impl IntoIterator<Item = Feature>) {
    let bits = features
        .into_iter()
        .fold(0, |bits, feature| bits | feature.bit());
    let _ = SELECTED.set(bits);
}

/// Whether the facade declares and binds `feature`: compiled in, and
/// selected when a selection was made.
#[must_use]
pub fn on(feature: Feature) -> bool {
    feature.compiled() && SELECTED.get().is_none_or(|bits| bits & feature.bit() != 0)
}

#[cfg(test)]
mod tests {
    use super::Feature;

    #[test]
    fn iced_features_name_the_widgets_they_enable() {
        assert_eq!(Feature::of_iced("canvas"), Some(Feature::Canvas));
        assert_eq!(Feature::of_iced("image"), Some(Feature::Image));
        assert_eq!(
            Feature::of_iced("image-without-codecs"),
            Some(Feature::Image)
        );
        assert_eq!(Feature::of_iced("qr_code"), Some(Feature::QrCode));
        assert_eq!(Feature::of_iced("wgpu"), None);
        assert_eq!(Feature::of_iced("advanced"), None);
    }

    /// The facade and the runtime binding it build with whichever widgets
    /// this build has; run with `--no-default-features` for none.
    #[test]
    fn the_facade_and_its_runtime_agree_on_the_compiled_widgets() {
        crate::facade().expect("the facade builds");
        crate::runtime_facades().expect("every declared item binds");
    }
}
