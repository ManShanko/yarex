//! Library for reading bundle resource files from the game engine Stingray.

#[macro_use]
mod error;
pub use error::StingrayResult as StingrayResult;
pub use error::StingrayError as StingrayError;

mod reader;
pub use reader::ReadBuffer as ReadBuffer;
pub use reader::BundleReader as BundleReader;

pub mod hash;

mod bundle;
pub use bundle::Bundle as Bundle;
pub use bundle::BundleVersion as BundleVersion;

pub mod file;
pub use file::BundleFile as BundleFile;

mod utility;
pub use utility::Patch as Patch;
pub use utility::format_bundle as format_bundle;
pub use utility::get_bundle_hash_patch as get_bundle_hash_patch;

mod consts {
    pub(crate) const ZLIB_CHUNK_SIZE: usize = 0x10000;
    pub(crate) const FILE_HEADER_SIZE: usize = 36;
}












