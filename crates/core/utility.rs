use std::convert::TryInto;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;
use std::path::{Path, PathBuf};
use std::io::{self, Read, Write, Seek, SeekFrom};
use std::fs;

use flate2::read::{ZlibEncoder, ZlibDecoder};
use stingray::file::FileKind;

use super::Index;

const MAGIC_WORD: u64 = 0x7865646e69736572;
const SAVE_VERSION: u16 = 1;

const KIBYTE: u64 = 1024;
const MIBYTE: u64 = KIBYTE * 1024;
const GIBYTE: u64 = MIBYTE * 1024;

pub fn size_to_string(num: u64) -> String {
    if num < KIBYTE*1000 {
        format!("{}.{:02} KiB", num / KIBYTE, ((num % KIBYTE) * 100) / KIBYTE)
    } else if num < MIBYTE*1000 {
        format!("{}.{:02} MiB", num / MIBYTE, ((num % MIBYTE) * 100) / MIBYTE)
    } else {
        format!("{}.{:02} GiB", num / GIBYTE, ((num % GIBYTE) * 100) / GIBYTE)
    }
}

pub fn format_bundle(hash: u64, patch: u16) -> String {
    match patch {
        0 => format!("{:016x}", hash),
        x => format!("{:016x}.patch_{:<03}", hash, x),
    }
}

pub fn get_vermintide_dir() -> io::Result<PathBuf> {
    steam::get_steam_apps().and_then(
        |apps| {
            for app in &apps {
                if app.app_id == 552500 {
                    let mut dir = app.install_dir.to_owned();
                    dir.push("bundle");
                    return Ok(dir);
                }
            }
            Err(io::Error::new(io::ErrorKind::NotFound, "failed to find Vermintide 2 directory"))
        })
}

pub fn load_reader(path: &Path) -> io::Result<Index> {
    #[cfg(feature = "serde_support")]
    {
        fs::OpenOptions::new()
            .read(true)
            .open(path)
            .and_then(|mut file| {
                let mut contents = Vec::new();
                file.read_to_end(&mut contents)?;
                if file.metadata()?.len() > 20 {
                    let magic_word = u64::from_le_bytes(contents[..8].try_into().unwrap());
                    if magic_word == MAGIC_WORD {
                        let save_version = u16::from_le_bytes(contents[8..10].try_into().unwrap());
                        if save_version == SAVE_VERSION {
                            // 10..18 is u64 hash which is unused for loads
                            let mut out = Vec::new();
                            let mut index = ZlibDecoder::new(&contents[18..]);
                            index.read_to_end(&mut out)?;

                            return bincode::deserialize(&out[..])
                                .map_err(|_| io::Error::new(io::ErrorKind::Other, "bincode deserialization failed"))
                        }
                    }
                }
                Err(io::Error::new(io::ErrorKind::Other, "incompatible save"))
            })
    }

    #[cfg(not(feature = "serde_support"))]
    Err(io::Error::new(io::ErrorKind::Other, "deserialization not enabled in build"))
}

pub fn save_reader(path: &Path, index: &Index, force_save: bool) -> io::Result<()> {
    #[cfg(feature = "serde_support")]
    {
        println!();
        print!("Saving to {}...", path.display());
        let start = std::time::Instant::now();

        let bin = bincode::serialize(&index)
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "bincode serialization failed"))?;

        let mut file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;

        let mut hasher = DefaultHasher::new();
        hasher.write(&bin);
        let hash = hasher.finish();

        let hash = if force_save {
            Some(hash)
        } else {
            let mut file_hash = [0; 8];
            let file_hash = match file.read(&mut file_hash) {
                Ok(8) => Some(u64::from_le_bytes(file_hash)),
                _ => None,
            };

            if file_hash != Some(hash) {
                Some(hash)
            } else {
                None
            }
        };

        if let Some(hash) = hash {
            let mut out = Vec::new();
            let mut comp = ZlibEncoder::new(&bin[..], flate2::Compression::new(4));
            comp.read_to_end(&mut out)?;
            //let out = bin;

            file.seek(SeekFrom::Start(0))?;
            file.write_all(&MAGIC_WORD.to_le_bytes())?;
            file.seek(SeekFrom::Start(8))?;
            file.write_all(&SAVE_VERSION.to_le_bytes())?;
            file.seek(SeekFrom::Start(10))?;
            file.write_all(&hash.to_le_bytes())?;
            file.seek(SeekFrom::Start(18))?;
            file.write_all(&out)?;
            file.set_len(18 + out.len() as u64)?;
        }

        let millis = start.elapsed().as_millis();
        println!(" finished in {}.{:02} seconds",  millis / 1000, (millis % 1000) / 10);
        Ok(())
    }

    #[cfg(not(feature = "serde_support"))]
    Err(io::Error::new(io::ErrorKind::Other, "serialization not enabled in build"))
}

pub fn print_extensions(index: &Index) {
    let mut extensions = Vec::<(u64, u64, u64, u64)>::with_capacity(1024*1024);

    for file in index.get_all_files() {
        match extensions.binary_search_by(|probe| probe.0.cmp(&file.ext_hash())) {
            Ok(i) => {
                let (_, _, _, total) = extensions.get_mut(i).unwrap();
                *total += 1;
            }
            Err(i) => extensions.insert(i, (file.ext_hash(), 0, 0, 1)),
        }
    }

    for file in index.get_unique_files() {
        if let Ok(i) = extensions.binary_search_by(|probe| probe.0.cmp(&file.ext_hash())) {
            let (_, _, unique, ..) = extensions.get_mut(i).unwrap();
            *unique += 1;
        }
    }

    for file in index.get_active_files() {
        if let Ok(i) = extensions.binary_search_by(|probe| probe.0.cmp(&file.ext_hash())) {
            let (_, active, ..) = extensions.get_mut(i).unwrap();
            *active += 1;
        }
    }

    let mut extensions = extensions.into_iter().map(|(hash, active, unique, total)| {
        let ext = match FileKind::with_hash(hash).as_str() {
            Some(x) => x.to_owned(),
            None => format!("{:16x}", hash),
        };
        (ext, active, unique, total)
    }).collect::<Vec<_>>();
    extensions.sort_by(|(a, ..), (b, ..)| a.cmp(b));

    let mut active = 0;
    let mut unique = 0;
    let mut total = 0;
    println!();
    let title = format!("{:<24} {:<7} {:<7} {}", "Extension", "Active", "Unique", "Total");
    println!("{}", title);
    println!("{}", "-".repeat(title.len()));
    for ext in &extensions {
        active += ext.1;
        unique += ext.2;
        total += ext.3;
        println!("{:<24} {:<7} {:<7} {}", ext.0, ext.1, ext.2, ext.3);
    }
    println!("{:>24} {:<7} {:<7} {}", "Total", active, unique, total);
}

pub fn print_info(index: &Index) -> Result<(), Box<dyn std::error::Error>> {
    let bundles = index.get_all_versions();
    let num_bundles = bundles.len();
    let mut num_base_bundles = 0;
    let mut all_bundles_size = 0;
    let mut base_bundles_size = 0;

    for bundle in bundles {
        all_bundles_size += bundle.size();
        if bundle.patch().is_base() {
            num_base_bundles += 1;
            base_bundles_size += bundle.size();
        }
    }

    let total_files = index.get_all_files();
    let num_total_files = total_files.len();
    let mut total_files_size = 0;
    for file in index.get_all_files() {
        total_files_size += file.size() as u64;
    }

    let unique_files = index.get_unique_files();
    let num_unique_files = unique_files.len();
    let mut unique_files_size = 0;
    for file in unique_files {
        unique_files_size += file.size() as u64;
    }

    let active_files = index.get_active_files();
    let num_active_files = active_files.len();
    let mut active_files_size = 0;
    for file in active_files {
        active_files_size += file.size() as u64;
    }

    println!();
    println!("Bundles: size on disk (count)");
    println!("  total: {} ({})", size_to_string(all_bundles_size), num_bundles);
    println!("  base: {} ({})", size_to_string(base_bundles_size), num_base_bundles);
    println!("  patches: {} ({})", size_to_string(all_bundles_size - base_bundles_size), num_bundles - num_base_bundles);
    println!();
    println!("Files: size uncompressed (count)");
    println!("  total: {} ({})", size_to_string(total_files_size), num_total_files);
    println!("  unique: {} ({})", size_to_string(unique_files_size), num_unique_files);
    println!("  active: {} ({})", size_to_string(active_files_size), num_active_files);
    Ok(())
}
