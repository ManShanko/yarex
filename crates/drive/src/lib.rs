// TODO look at timing file system access instead of OS specific solutions for
// identifying storage type (HDD, SSD)

//! Small library for getting physical offset of files and drive type to
//! optimize read patterns.
//!
//! Currently only supports Windows.
use std::path::Path;
use std::fs::File;

#[cfg(target_os = "windows")]
mod windows;

/// Get physical offset of File.
///
/// # Example
///
/// ```no_run
/// use std::fs::File;
///
/// let fd = File::open("test").unwrap();
///
/// match drive::file_offset(&fd) {
///     Some(offset) => println!("file offset is {}", offset),
///     None => println!("unsupported operation on this system"),
/// }
/// ```
pub fn file_offset(fd: &File) -> Option<u64> {
    file_offset_(fd)
}

/// Attempts to find if given path is on an SSD or not.
///
/// # Example
///
/// ```
/// use std::path::Path;
///
/// #[cfg(target_os = "windows")]
/// let dir = if cfg!(target_os = "windows") {
///     r"C:\"
/// } else {
///     "/"
/// };
///
/// match drive::in_ssd(dir) {
///     Some(true) => println!("{} is stored on an SSD", dir),
///     Some(false) => println!("{} is not stored on an SSD", dir),
///     None => println!("unsupported operation on this system"),
/// }
/// ```
pub fn in_ssd<T: AsRef<Path>>(bundle: T) -> Option<bool> {
    in_ssd_(bundle.as_ref())
}



#[cfg(target_os = "windows")]
fn file_offset_(fd: &File) -> Option<u64> {
    windows::file_offset(fd)
}

#[cfg(target_os = "windows")]
fn in_ssd_(path: &Path) -> Option<bool> {
    windows::in_ssd(path)
}



#[cfg(not(target_os = "windows"))]
fn file_offset_(_fd: &File) -> Option<u64> {
    None
}

#[cfg(not(target_os = "windows"))]
fn in_ssd_(_path: &Path) -> Option<bool> {
    None
}
