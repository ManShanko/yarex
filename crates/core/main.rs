use std::path::PathBuf;
use std::env;
use std::fs;
use std::fs::File;
use std::fs::OpenOptions;

mod utility;
use utility::{
    save_reader,
    print_extensions,
    print_info,
    get_vermintide_dir,
};
mod reader;
use reader::Index as Index;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const PROGRAM_NAME: &str = env!("CARGO_PKG_NAME");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = env::args_os().collect::<Vec<_>>();
    let num_args = args.len() - 1;

    // opt_value_from_fn
    let mut pico = pico_args::Arguments::from_vec(args);
    let long_help = pico.contains("--help");
    let short_help = pico.contains("-h");

    if num_args == 0 || long_help || short_help {
            println!("{} {}", PROGRAM_NAME, VERSION);
            println!("ManShanko, github.com/manshanko");
            if long_help {
                println!();
                println!("Resource extractor for applications made with the Stingray Engine.");
            }
            println!();
            println!("switches:");
            println!("  -h, --help              Show help.");
            println!("      --hash <STRING>     Hash string.");
            println!("      --benchmark         Disable extracting files to disk.");
            println!("      --buffered          Force buffered IO.");
            println!("      --bundle <FILE>     Uncompress bundle to --out");
            println!("      --dir-bundle <FILE> Uncompress bundle from --path to --out");
            println!("      --extensions        List file type counts.");
            println!("  -e, --extract <GLOB>    Glob match for extracting files.");
            println!("  -f, --force             Force new cache creation.");
            println!("      --hash-fallback     Fallback to file hash if file name is unknown.");
            println!("  -i, --info              Print index info.");
            println!("      --no-save           Disable saving cache.");
            println!("  -c, --cache <FILE>      Set cache file to save/load work with.");
            println!("  -k, --keys <FILE>       Set keys file to use when doing reverse lookup with hashes.");
            println!("  -o, --out <DIR>         Set output directory.");
            println!("  -d, --dir <DIR>         Set input directory.");
            println!("  -t, --threads <COUNT>   Set thread count.");
    } else if let Ok(word) = pico.value_from_str::<_, String>("--hash") {
        let hash = stingray::hash::murmur_hash(word.as_bytes());
        let half = (hash & 0xFFFFFFFF) as u32;
        println!("{:016x}", hash);
        println!("{:08x}", half);
        println!("byte swap:");
        println!("{:016x}", hash.swap_bytes());
        println!("{:08x}", half.swap_bytes());
    } else {
        let dir: PathBuf = match pico.value_from_str("--dir")
            .or_else(|_| pico.value_from_str("-d"))
            .or_else(|_| get_vermintide_dir())
        {
            Ok(dir) if dir.exists() => dir,
            Ok(dir) => {
                println!("Invalid target directory \"{}\"", dir.display());
                return Ok(());
            }
            Err(_) => {
                println!("Unable to find target directory");
                println!("Use -d to change target directory");
                return Ok(());
            }
        };

        let out_dir: PathBuf = pico.value_from_str("--out")
            .or_else(|_| pico.value_from_str("-o"))
            .unwrap_or_else(|_|
                match env::current_dir() {
                    Ok(mut out) => {
                        out.push("out");
                        out
                    }
                    _ => PathBuf::from("out"),
                });

        let index_file: PathBuf = pico.value_from_str("--cache")
            .or_else(|_| pico.value_from_str("-c"))
            .unwrap_or_else(|_| format!("{}.idx", PROGRAM_NAME).into());

        let num_threads = pico.value_from_str("--threads")
            .or_else(|_| pico.value_from_str("-t"))
            .unwrap_or_else(|_| num_cpus::get());

        let pattern: Option<String> = match pico.value_from_str("--extract")
            .or_else(|_| pico.value_from_str("-e"))
        {
            Err(pico_args::Error::OptionWithoutAValue(o)) => {
                println!("Missing value for \"{}\"", o);
                return Ok(());
            }
            Ok(v) => Some(v),
            _ => None,
        };

        let bundle: Option<(PathBuf, PathBuf)> = pico.value_from_str("--bundle")
            .map(|bundle: PathBuf| {
                let out = match bundle.file_name() {
                    Some(name) => out_dir.join(name),
                    None => out_dir.join(&bundle),
                };
                (bundle, out)
            })
            .or_else(|_| pico.value_from_str("--dir-bundle")
                .map(|bundle: PathBuf| {
                    let out = match bundle.file_name() {
                        Some(name) => out_dir.join(name),
                        None => out_dir.join(&bundle),
                    };
                    (dir.join(bundle), out)
                })
            )
            .ok();

        let keys: PathBuf = pico.value_from_str("--keys")
            .or_else(|_| pico.value_from_str("-k"))
            .unwrap_or_else(|_| "dictionary.txt".into());

        let benchmark      = pico.contains("--benchmark");
        let force_index    = pico.contains("--force") || pico.contains("-f");
        let force_buffered = pico.contains("--buffered");
        let hash_fallback  = pico.contains("--hash-fallback");
        let do_extensions  = pico.contains("--extensions");
        let do_info        = pico.contains("--info") || pico.contains("-i");
        let no_save        = pico.contains("--no-save");

        if let Some((bundle_in, bundle_out)) = bundle {
            if let Ok(mut fd) = File::open(bundle_in) {
                fs::create_dir_all(bundle_out.parent().unwrap()).unwrap();
                let mut target = OpenOptions::new()
                    .write(true)
                    .truncate(true)
                    .create(true)
                    .open(bundle_out).unwrap();

                reader::decompress_bundle(&mut fd, &mut target);
            }
        } else {
            if num_threads == 0 {
                println!("No threads available.");
                println!("Exiting...");
            }

            let index_path = match force_index {
                false => Some(index_file.as_ref()),
                true => None,
            };
            let mut index = reader::load_index(&dir, index_path, num_threads, !force_buffered)?;
            if keys.exists() {
                index.load_keys(&keys);
            }

            if let Some(pattern) = pattern {
                let out = if benchmark || (cfg!(debug_assertions) && !pico.contains("--debug-extract")) {
                    None
                } else {
                    Some(out_dir.as_path())
                };

                // while the speed is almost as fast as OS buffering it removes
                // OS caching of repeated reads that don't use unbuffered reads
                //
                // overall needs more tweaking to use outside of indexing
                let unbuffered = false;
                index.extract_files_with_progress(out, &pattern, num_threads, unbuffered, hash_fallback)?;
            }

            if do_extensions {
                print_extensions(&index);
            }

            if do_info {
                print_info(&index)?;
            }

            // if force_index is false then save_reader will do hash comparison
            // for final check to avoid compression if nothing has changed
            if index.dirty() && !no_save {
                save_reader(&index_file, &index, force_index)?;
            }
        }
    }

    Ok(())
}

