// rawr
// iced's color_palette: a generic picker's view, `impl Widget<C>`, is
// compiled once per color space it is used at, a script type or a crate's
// (palette's `Hsl` there, `Lab` here). It reads the associated constants
// of the space's trait through its generic parameter, `C::RANGES` and
// `C::LABEL`, as rustc does, and makes a new color with `C::new` inside a
// nested generic function's callback.

use std::marker::PhantomData;
use std::ops::RangeInclusive;

use events::color::Hsl as Lab;
use events::widgets::{button, column, text, Framed, Widget};

trait Space: Sized {
    const LABEL: &'static str;
    const RANGES: [RangeInclusive<f64>; 3];

    fn new(a: f32, b: f32, c: f32) -> Self;

    fn components(&self) -> [f32; 3];
}

#[derive(Debug, Clone, Copy, Default)]
struct Rgb {
    r: f32,
    g: f32,
    b: f32,
}

impl Space for Rgb {
    const LABEL: &'static str = "RGB";
    const RANGES: [RangeInclusive<f64>; 3] = [0.0..=1.0, 0.0..=1.0, 0.0..=1.0];

    fn new(r: f32, g: f32, b: f32) -> Self {
        Rgb { r, g, b }
    }

    fn components(&self) -> [f32; 3] {
        [self.r, self.g, self.b]
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Hsl {
    h: f32,
    s: f32,
    l: f32,
}

impl Space for Hsl {
    const LABEL: &'static str = "HSL";
    const RANGES: [RangeInclusive<f64>; 3] = [0.0..=360.0, 0.0..=1.0, 0.0..=1.0];

    fn new(h: f32, s: f32, l: f32) -> Self {
        Hsl { h, s, l }
    }

    fn components(&self) -> [f32; 3] {
        [self.h, self.s, self.l]
    }
}

impl Space for Lab {
    const LABEL: &'static str = "Lab";
    const RANGES: [RangeInclusive<f64>; 3] = [0.0..=100.0, -128.0..=127.0, -128.0..=127.0];

    fn new(l: f32, a: f32, b: f32) -> Self {
        Lab {
            hue: l,
            saturation: a,
            lightness: b,
            standard: PhantomData,
        }
    }

    fn components(&self) -> [f32; 3] {
        [self.hue, self.saturation, self.lightness]
    }
}

#[derive(Default)]
struct Picker<C: Space> {
    space: PhantomData<C>,
}

impl<C: Space + Copy> Picker<C> {
    fn view(&self, color: C, label: &'static str) -> impl Widget<C> {
        let [c1, c2, c3] = color.components();
        let [r1, r2, r3] = C::RANGES;

        fn step<C: Clone>(
            range: RangeInclusive<f64>,
            component: f32,
            update: impl Fn(f32) -> C,
        ) -> C {
            let next = f64::from(component) + (range.end() - range.start()) / 4.0;
            update(next.min(*range.end()) as f32)
        }

        column![
            text(format!("{} {}..={}", C::LABEL, r1.start(), r1.end())),
            button(label).on_press(step(r1, c1, move |v| C::new(v, c2, c3))),
            text(format!("{:?} {:?}", r2.end(), r3.end())),
        ]
    }
}

struct Palette {
    rgb: Rgb,
    hsl: Hsl,
    rgb_picker: Picker<Rgb>,
    hsl_picker: Picker<Hsl>,
    lab: Lab,
    lab_picker: Picker<Lab>,
}

impl Default for Palette {
    fn default() -> Self {
        Palette {
            rgb: Rgb::default(),
            hsl: Hsl::default(),
            rgb_picker: Picker::default(),
            hsl_picker: Picker::default(),
            lab: Lab::default(),
            lab_picker: Picker::default(),
        }
    }
}

#[derive(Debug, Clone)]
enum Message {
    Rgb(Rgb),
    Hsl(Hsl),
    Lab(Lab),
}

impl Palette {
    fn update(&mut self, message: Message) {
        match message {
            Message::Rgb(rgb) => self.rgb = rgb,
            Message::Hsl(hsl) => self.hsl = hsl,
            Message::Lab(lab) => self.lab = lab,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            self.rgb_picker
                .view(self.rgb, "Increment")
                .map(Message::Rgb),
            self.hsl_picker
                .view(self.hsl, "Increment")
                .map(Message::Hsl),
            self.lab_picker
                .view(self.lab, "Decrement")
                .map(Message::Lab),
            text(format!(
                "{:?} {:?} {:?}",
                self.rgb.components(),
                self.hsl.components(),
                self.lab.components()
            )),
        ]
    }
}

fn main() {
    events::widgets::run(Palette::update, Palette::view);
}
