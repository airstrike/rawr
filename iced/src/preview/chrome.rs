//! Rawr-owned presentation for preview-host controls and diagnostics.

pub(super) const CONTENT_ID: &str = "rawr-preview-content";
pub(super) const DIAGNOSTIC_ID: &str = "rawr-preview-diagnostic";
pub(super) const FOOTER_ID: &str = "rawr-preview-footer";
pub(super) const STATUS_ID: &str = "rawr-preview-status";
pub(super) const SELECTION_ID: &str = "rawr-preview-selection";
pub(super) const ACTIONS_ID: &str = "rawr-preview-actions";
pub(super) const SELECTOR_ID: &str = "rawr-preview-selector";
pub(super) const CLOSE_ID: &str = "rawr-preview-close";
pub(super) const PREVIOUS_ID: &str = "rawr-preview-previous";
pub(super) const NEXT_ID: &str = "rawr-preview-next";
pub(super) const POSITION_ID: &str = "rawr-preview-position";
pub(super) const THEME_ID: &str = "rawr-preview-theme";
pub(super) const RESET_ID: &str = "rawr-preview-reset";
pub(super) const SCREENSHOT_ID: &str = "rawr-preview-screenshot";
pub(super) const FOOTER_MAX_HEIGHT: f32 = 32.0;

// Industrial Y2K, matching RawrGallery's Ink sheet: graphite grays with one
// hot-pink accent and square corners.
const BACKGROUND: iced::Color = iced::Color::from_rgb8(0x14, 0x14, 0x16);
const HOVERED: iced::Color = iced::Color::from_rgb8(0x24, 0x24, 0x28);
const BORDER: iced::Color = iced::Color::from_rgb8(0x3a, 0x3a, 0x40);
const TEXT: iced::Color = iced::Color::from_rgb8(0xe4, 0xe4, 0xe8);
const MUTED: iced::Color = iced::Color::from_rgb8(0x9a, 0x9a, 0xa3);
const ACCENT: iced::Color = iced::Color::from_rgb8(0xff, 0x2e, 0x93);

pub(crate) fn theme() -> iced::Theme {
    iced::Theme::custom(
        "Rawr preview chrome",
        iced::theme::palette::Seed {
            background: BACKGROUND,
            text: TEXT,
            primary: ACCENT,
            success: MUTED,
            warning: MUTED,
            danger: ACCENT,
        },
    )
}

pub(crate) const fn accent() -> iced::Color {
    ACCENT
}

pub(crate) fn surface(_theme: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        text_color: Some(TEXT),
        background: Some(BACKGROUND.into()),
        ..iced::widget::container::Style::default()
    }
}

pub(crate) fn button(
    _theme: &iced::Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    use iced::widget::button::{Status, Style};

    let background = match status {
        Status::Hovered | Status::Pressed => Some(HOVERED.into()),
        Status::Active | Status::Disabled => None,
    };
    Style {
        background,
        text_color: if matches!(status, Status::Disabled) {
            BORDER
        } else {
            MUTED
        },
        border: iced::Border {
            color: if matches!(status, Status::Hovered | Status::Pressed) {
                ACCENT
            } else {
                iced::Color::TRANSPARENT
            },
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Style::default()
    }
}

pub(crate) fn pick_list(
    _theme: &iced::Theme,
    status: iced::widget::pick_list::Status,
) -> iced::widget::pick_list::Style {
    use iced::widget::pick_list::{Status, Style};

    let interactive = matches!(status, Status::Hovered | Status::Opened { .. });
    Style {
        text_color: if matches!(status, Status::Disabled) {
            BORDER
        } else {
            MUTED
        },
        placeholder_color: MUTED,
        handle_color: if interactive { TEXT } else { MUTED },
        background: if interactive { HOVERED } else { BACKGROUND }.into(),
        border: iced::Border {
            color: if interactive { ACCENT } else { BORDER },
            width: 1.0,
            radius: 0.0.into(),
        },
    }
}

pub(crate) fn divider(_theme: &iced::Theme) -> iced::widget::rule::Style {
    iced::widget::rule::Style {
        color: BORDER,
        radius: 0.0.into(),
        fill_mode: iced::widget::rule::FillMode::Full,
        snap: true,
    }
}

/// Square status card shared by the reload overlay and host notes.
pub(crate) fn card(emphasis: bool) -> impl Fn(&iced::Theme) -> iced::widget::container::Style {
    move |_theme| iced::widget::container::Style {
        text_color: Some(TEXT),
        background: Some(BACKGROUND.into()),
        border: iced::Border {
            color: if emphasis { ACCENT } else { BORDER },
            width: 1.0,
            radius: 0.0.into(),
        },
        ..iced::widget::container::Style::default()
    }
}
