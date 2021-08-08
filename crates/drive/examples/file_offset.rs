use std::fs::File;

use drive::file_offset;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = "Cargo.toml";
    let fd = File::open(file)?;

    match file_offset(&fd) {
        Some(offset) => println!("{} has offset {}", file, offset),
        None => println!("{} has unknown offset", file),
    }

    Ok(())
}
