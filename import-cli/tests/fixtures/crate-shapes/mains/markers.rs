// rawr
use natives::badge::{self, badge, Badge, Wrap};

fn ghost() -> badge::Ghost {
    badge::Ghost
}

fn main() {
    let marker = badge::Secondary;
    let copy = marker;
    println!(
        "{:?} {} {}",
        marker,
        marker == copy,
        marker.clone() == badge::Secondary
    );
    println!("{}", badge("Secondary").style(badge::Secondary).draw());
    let style: badge::StyleFn = ghost().into();
    let drawn: Badge = badge("Ghost").style(style);
    println!("{}", drawn.draw());
    match marker {
        badge::Secondary => println!("matched"),
    }
    let wraps: Vec<Wrap> = vec![1, 2].into_iter().map(Wrap).collect();
    println!("{:?}", wraps);
}
