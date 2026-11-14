// rawr
use natives::capabilities::{Capabilities, Format};

fn main() {
    let capabilities = natives::capabilities::capabilities();
    let format = capabilities
        .formats
        .iter()
        .copied()
        .find(Format::is_srgb)
        .or_else(|| capabilities.formats.first().copied())
        .expect("format");
    println!("{format:?} {}", capabilities.formats.len());
    println!("{}", capabilities.tag.is_some());
    let name = capabilities.tag.as_ref().map(|tag| tag.name.clone());
    println!("{name:?} {}", capabilities.count);
    let Capabilities { formats, .. } = capabilities;
    println!("{}", formats.len());
}
