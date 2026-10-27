enum Shape {
    Rect { w: i64, h: i64 },
    Tagged(String, Option<(i64, Vec<i64>)>),
}

fn main() {
    let mut shapes = vec![Shape::Rect { w: 1, h: 2 }, Shape::Tagged(String::from("t"), Some((3, vec![4])))];
    for shape in shapes.iter_mut() {
        match shape {
            Shape::Rect { w, h } => {
                *w *= 10;
                *h += *w;
            }
            Shape::Tagged(name, inner) => {
                name.push('!');
                if let Some((n, list)) = inner {
                    list.push(*n);
                    *n = list.len() as i64;
                }
            }
        }
    }
    for shape in &shapes {
        match shape {
            Shape::Rect { w, h } => println!("rect {w} {h}"),
            Shape::Tagged(name, inner) => println!("tagged {name} {inner:?}"),
        }
    }
}
