use drive::in_ssd;

fn main() {
    let dir = r"C:\Program Files (x86)\Steam\steamapps\common\Warhammer Vermintide 2\bundle";
    match in_ssd(dir) {
        Some(true) => println!("{} is in an SSD", dir),
        Some(false) => println!("{} is in not an SSD", dir),
        None => println!("could not find storage device type"),
    }
}
