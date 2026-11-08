// rawr
use natives::pages::{self, Block};

static PAGE: &[Block] = &[
    Block::Md("intro"),
    Block::Group {
        title: "Examples",
        children: &[
            Block::Example {
                id: "dot",
                description: Some("a dot"),
            },
            Block::Group {
                title: "Nested",
                children: &[Block::Md("deep")],
            },
        ],
    },
];

fn main() {
    println!("{}", pages::count(PAGE));
    let group = Block::Group {
        title: "Built",
        children: &[Block::Example {
            id: "built",
            description: None,
        }],
    };
    println!("{} {}", pages::heading(&group), pages::count(&[group]));
    println!("{}", pages::heading(&PAGE[1]));
}
