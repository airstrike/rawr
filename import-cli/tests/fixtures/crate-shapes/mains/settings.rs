// rawr
// swash's `Setting<u16>` and `Setting<f32>` each have a `parse_list` of
// their own: a program that imports the type, and doesn't call it, as each
// iced program imports swash.
use natives::form::Setting;

fn main() {
    let setting = Setting(3_u16);
    println!("{}", setting.0);
}
