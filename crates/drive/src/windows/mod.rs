use std::path::Path;
use std::fs::File;

mod ioctl;
mod wmi;

pub fn file_offset(fd: &File) -> Option<u64> {
    ioctl::starting_vcn(fd)
}

pub fn in_ssd(path: &Path) -> Option<bool> {
    let drive = match get_drive_letter(path) {
        Some(letter) => letter,
        None => return None,
    };

    wmi::co_init().ok()?;

    // doesn't handle multiple drives mounted to the same volume (e.g. C:\)
    let is_ssd = if let Some(index) = wmi::get_device_index(drive.as_ref()).ok()? {
        wmi::get_media_type(index).ok()??
    } else { wmi::DriveKind::Unknown };

    wmi::co_exit();

    match is_ssd {
        wmi::DriveKind::SSD => Some(true),
        wmi::DriveKind::HDD => Some(false),
        wmi::DriveKind::Unknown => None,
    }
}

fn get_drive_letter(path: &Path) -> Option<String> {
    let mut path_iter = path.to_str()?.chars();
    let letter = path_iter.next()?.to_uppercase();
    let colon = path_iter.next()?;

    if colon == ':' {
        // C:
        Some(format!("{}{}", letter, colon))
    } else {
        // \\?\C:
        let letter = path_iter.nth(2)?;
        let colon = path_iter.next()?;
        if colon == ':' {
            Some(format!("{}{}", letter, colon))
        } else {
            None
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drive_letter() {
        assert_eq!(get_drive_letter(Path::new(r"C:\test\directory")), Some("C:".into()));
        assert_eq!(get_drive_letter(Path::new(r"\\?\C:\test\directory")), Some("C:".into()));
        assert_eq!(get_drive_letter(Path::new(r"\\?\test\directory")), None);
    }
}

