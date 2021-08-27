use std::path::Path;
use std::fmt;

/// Create bundle file name from hash and patch.
///
/// # Example
///
/// ```
/// use stingray::format_bundle;
/// use stingray::get_bundle_hash_patch;
///
/// let (hash, patch) = get_bundle_hash_patch("0123456789abcdef.patch_006").unwrap();
/// assert_eq!(format_bundle(hash, patch).as_str(), "0123456789abcdef.patch_006");
/// ```
pub fn format_bundle<T: Into<Patch>>(hash: u64, patch: T) -> String {
    format_bundle_(hash, patch.into())
}

#[doc(hidden)]
fn format_bundle_(hash: u64, patch: Patch) -> String {
    match patch.get() {
        None => format!("{:016x}", hash),
        Some(x) => format!("{:016x}.patch_{:<03}", hash, x),
    }
}

/// Try to parse bundle path into u64 hash and u16 patch number.
///
/// # Example
///
/// ```
/// use stingray::Patch;
/// use stingray::get_bundle_hash_patch;
///
/// assert_eq!(get_bundle_hash_patch("0123456789abcdef"), Some((0x0123456789abcdef, Patch::new_base())));
/// assert_eq!(get_bundle_hash_patch("0123456789abcdef.patch_006"), Some((0x0123456789abcdef, Patch::new(6))));
/// assert_eq!(get_bundle_hash_patch("invalid.txt"), None);
/// ```
pub fn get_bundle_hash_patch<T: AsRef<Path>>(bundle: T) -> Option<(u64, Patch)> {
    get_bundle_hash_patch_(bundle.as_ref())
}

#[doc(hidden)]
fn get_bundle_hash_patch_(path: &Path) -> Option<(u64, Patch)> {
    let name = path.file_stem()?.to_str()?;
    if name.len() != 16 { return None; }
    let hash = u64::from_str_radix(name, 16).ok()?;
    let patch = match path.extension() {
        Some(ext) if ext.len() == 9 => {
            let s = ext.to_str()?;
            s[7..9].parse::<u16>().ok()?
        },
        Some(_) => return None,
        None => 0,
    };

    Some((hash, Patch::new(patch)))
}

/// Wrapper around patch numbers for bundles.
#[repr(transparent)]
#[cfg_attr(feature = "serde_support", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Patch(u16);

impl Patch {
    /// Create new Patch.
    ///
    /// Passing 0 is the same as calling [new_base](Patch::new_base).
    pub fn new(num: u16) -> Self {
        assert!(num < 1000);

        Self(num)
    }

    /// Create new Patch base.
    pub fn new_base() -> Self {
        Self(0)
    }

    pub fn is_base(&self) -> bool {
        self.0 == 0
    }

    /// Get patch number.
    pub fn get(&self) -> Option<u16> {
        match self.0 {
            0 => None,
            x => Some(x),
        }
    }
}

impl From<u16> for Patch {
    fn from(num: u16) -> Self {
        Self::new(num)
    }
}

impl fmt::Display for Patch {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}




