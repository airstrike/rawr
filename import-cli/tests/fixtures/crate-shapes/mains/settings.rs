// rawr
use natives::form::Setting;

fn main() {
    let setting = Setting(3_u16);
    println!("{:?} {}", setting, setting.0 + 1);
}
