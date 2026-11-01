// rawr
// iced's geometry: `SolidVertex2D { position, color }` fills a field of an
// array type and one of a crate type whose own field is private, which
// only the crate's `pack` makes. The program is `run((), view)`: its state
// is the unit, its update is the crate's own impl for `()`, and its view
// gives widgets that borrow string literals for as long as the unit is
// lent.

use events::advanced::graphics::color;
use events::advanced::graphics::mesh::{self, Mesh, SolidVertex2D};
use events::widgets::{center, column, text, Widget};
use events::Never;

fn rainbow(width: f32) -> Mesh {
    let color_r = [1.0, 0.0, 0.0, 1.0];
    let posn_center = [width / 2.0, width / 2.0];
    Mesh::Solid {
        buffers: mesh::Indexed {
            vertices: vec![
                SolidVertex2D {
                    position: posn_center,
                    color: color::pack([1.0, 1.0, 1.0, 1.0]),
                },
                SolidVertex2D {
                    position: [0.0, 0.0],
                    color: color::pack(color_r),
                },
                SolidVertex2D {
                    position: [width, 0.0],
                    color: color::pack([0.5, 0.25, 0.0, 1.0]),
                },
            ],
            indices: vec![0, 1, 2],
        },
        offset: [width, 1.5],
    }
}

fn view(_state: &()) -> impl Widget<Never> {
    let content = column![
        text("rainbow"),
        "In this example we draw a custom widget Rainbow",
        "Move your cursor over it",
    ]
    .padding(20);

    center(content)
}

fn main() {
    let mesh = rainbow(4.0);
    println!("{}", mesh::describe(&mesh));
    let vertex = SolidVertex2D {
        position: [1.0, 2.0],
        color: color::pack([0.5, 0.25, 0.0, 1.0]),
    };
    println!("{:?} {:?}", vertex.position, vertex.color);
    events::widgets::run((), view);
}
