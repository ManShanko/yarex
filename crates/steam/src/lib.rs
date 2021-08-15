//! # steam
//!
//! Crate for getting the install location of Steam and any Steam apps.
//!
//! **NOTE:** Currently only tested on Windows

use std::{fs, io, ffi};
use std::path::PathBuf;
mod vdf;

/// Gets Steam install location.
///
/// # Example
///
/// ```
/// let steam_path = steam::get_steam_dir();
/// ```
///
/// NOTE: Only Windows is supported, but a best effort was made for Linux and Mac.
#[cfg(target_os = "windows")]
pub fn get_steam_dir() -> io::Result<PathBuf> {
    use std::convert::TryInto;
    use std::mem::size_of;
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::ffi::OsStringExt;

    use winapi::um::winreg::RegGetValueW;
    use winapi::um::winreg::HKEY_CURRENT_USER;
    use winapi::um::winreg::RRF_RT_REG_SZ;

    const BUFFER_SIZE: usize = 1024;

    let mut buffer: [u16; BUFFER_SIZE] = [0; BUFFER_SIZE];
    let mut size = size_of::<[u16; BUFFER_SIZE]>().try_into().unwrap();
    let mut kind = 0;
    let path = match unsafe {
        if RegGetValueW(
            HKEY_CURRENT_USER,
            OsString::from("SOFTWARE\\Valve\\Steam\0").encode_wide().collect::<Vec<_>>().as_ptr() as *const _,
            OsString::from("SteamPath\0").encode_wide().collect::<Vec<_>>().as_ptr() as *const _,
            RRF_RT_REG_SZ,
            &mut kind,
            buffer.as_mut_ptr() as *mut _,
            &mut size,
        ) == 0 {
            let len = (size as usize - 1) / 2;
            let path = PathBuf::from(OsString::from_wide(&buffer[..len]));

            if path.exists() {
                Some(path)
            } else { None }
        } else { None }
    } {
        Some(path) => path,
        None => return Err(io::Error::new(io::ErrorKind::NotFound, "steam was not found")),
    };

    Ok(path)
}

#[cfg(target_os = "macos")]
pub fn get_steam_dir() -> io::Result<PathBuf> {
    Ok(PathBuf::from("~/Library/Application Support/Steam"))
}

#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
pub fn get_steam_dir() -> io::Result<PathBuf> {
    Ok(PathBuf::from("~/.steam/steam"))
}

pub struct SteamApp {
    pub app_id: u64,
    pub size: u64,
    pub name: String,
    pub install_dir: PathBuf,
}

/// Search Steam libraries and return a list of installed Steam applications.
///
/// # Example
///
///```
///match steam::get_steam_apps() {
///    Ok(apps) => {
///        for i in 0..apps.len() {
///            let app = &apps[i];
///            println!("Information for app \"{}\":\n", app.name);
///            println!("  app_id: {}\n", app.app_id);
///            println!("  size: {}\n", app.size);
///            println!("  install_dir: {}\n", app.install_dir.display());
///        }
///    },
///    Err(e) => {
///        println!("Steam is not installed");
///    },
///}
///```
pub fn get_steam_apps() -> io::Result<Vec<SteamApp>> {
    let mut out = Vec::<SteamApp>::new();
    let steam_path = get_steam_dir()?;
    let dir = steam_path.join("steamapps");
    let lib_file = PathBuf::from(&dir).join("libraryfolders.vdf");
    let mut library_paths = match vdf::get_library_folders(&fs::read_to_string(&lib_file)?) {
        Ok(lib) => lib,
        Err(e) => return Err(io::Error::new(io::ErrorKind::Other, e)),
    }.folders;
    for lib in library_paths.iter_mut() {
        lib.push("steamapps");
    }
    library_paths.insert(0, dir);
    for library in library_paths {
        for file in fs::read_dir(&library)? {
            let path = file?.path();
            if path.extension() == Some(ffi::OsStr::new("acf")) {
                let app = match vdf::read_app_manifest(&fs::read_to_string(&path)?) {
                    Ok(dir) => dir,
                    Err(e) => return Err(io::Error::new(io::ErrorKind::Other, e)),
                };
                let mut install_dir = library.join("common");
                install_dir.push(app.installdir);
                out.push(SteamApp {
                    app_id: app.appid,
                    name: app.name,
                    install_dir,
                    size: app.SizeOnDisk,
                });
            }
        }
    }
    Ok(out)
}

/// Convenience function to iterate through [`get_steam_apps`](get_steam_apps) for a specific `app_id`.
pub fn get_app(app_id: u64) -> io::Result<SteamApp> {
    match get_steam_apps() {
        Ok(apps) => {
            for app in apps {
                if app.app_id == app_id {
                    return Ok(app);
                }
            }
            Err(io::Error::new(io::ErrorKind::NotFound, "failed to find steam app"))
        }
        Err(e) => Err(e),
    }
}



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steam_exists() {
        assert!(get_steam_dir().is_ok());
    }

    #[test]
    fn steam_apps() -> io::Result<()> {
        let apps = get_steam_apps()?;
        assert!(apps.len() > 0);
        for i in 0..apps.len() {
            let app = &apps[i];
            assert!(app.app_id > 0);
            assert!(app.name.capacity() > 0);
            assert!(app.install_dir.exists());
            assert!(app.size > 0);
        }
        Ok(())
    }
}
