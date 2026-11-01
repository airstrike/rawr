// rawr
// iced's solar_system: `canvas::LineDash { segments: &[3.0, 6.0], offset:
// 0 }`, a struct of public fields re-exported from another crate, one of
// them a borrowed slice, built inside a `Stroke { .., ..Stroke::default() }`.

use events::canvas::{self, Stroke};

fn main() {
    let dashed = Stroke {
        width: 2.0,
        line_dash: canvas::LineDash {
            offset: 1,
            segments: &[3.0, 6.0],
        },
        ..Stroke::default()
    };
    println!("{}", dashed.describe());
    let dash = canvas::LineDash {
        segments: &[0.5],
        offset: 0,
    };
    println!("{:?} {}", dash.segments, dash.offset);
    println!("{}", Stroke::default().describe());
}
