// rawr
use iced::Color;
use iced::Theme;
use iced::theme::palette::Seed;

pub fn midnight() -> Theme {
    Theme::custom(
        "Midnight package",
        Seed {
            background: Color::from_rgb8(0x10, 0x13, 0x1a),
            text: Color::from_rgb8(0xe7, 0xea, 0xf0),
            primary: Color::from_rgb8(0x8b, 0xb8, 0xff),
            success: Color::from_rgb8(0x78, 0xdb, 0xa9),
            warning: Color::from_rgb8(0xf4, 0xcc, 0x67),
            danger: Color::from_rgb8(0xff, 0x7d, 0x8b),
        },
    )
}
