//! File type handler.
//!
//! Currently has a custom implementation for `lua` and `texture` files.
//! Other file types use a generic implementation that copies the data raw.

use std::convert::TryInto;
use std::io::Write;

use crate::hash::murmur_hash64a;

#[macro_use]
mod macros;
mod lua;
mod texture;
mod wwise_dep;

// convenience one use macro for generating all the code
// currently handles mod manually in the macro since it is a reserved keyword and
// stringify converts r#mod to "r#mod" instead of "mod"
file_kinds! {
    animation,
    animation_curves,
    bik,
    blend_set,
    bones,
    chroma,
    common_package,
    config,
    data,
    entity,
    flow,
    font,
    ini,
    ivf,
    keys,
    level,
    lua,
    material,
    //r#mod,
    mouse_cursor,
    navdata,
    network_config,
    package,
    particles,
    physics_properties,
    render_config,
    scene,
    shader,
    shader_library,
    shader_library_group,
    shading_environment,
    shading_environment_mapping,
    slug,
    state_machine,
    strings,
    texture,
    tome,
    unit,
    vector_field,
    wwise_bank,
    wwise_dep,
    wwise_metadata,
    wwise_stream,
}

/// Trait for implementing a file type processor.
pub trait FileReader<'a> {
    fn decompile(&mut self, out: &mut dyn Write) -> crate::StingrayResult<usize>;

    fn path(&self) -> (Option<&str>, Option<&str>) {
        (None, None)
    }
}

// default interface for files with no implementation
// writes file contents raw
struct UnknownFile<'a> {
    buffer: &'a [u8],
}

impl<'a> FileReader<'a> for UnknownFile<'a> {
    fn decompile(&mut self, out: &mut dyn Write) -> crate::StingrayResult<usize> {
        out.write_all(&self.buffer[36..])?;
        Ok(self.buffer[36..].len())
    }
}

#[allow(non_snake_case, non_upper_case_globals)]
mod FileFlags {
    pub const BadOffset: u8 = 0b00000001;
    pub const Deleted: u8   = 0b00000010;
    pub const Deleted2: u8  = 0b00000100;
}

/// File index entry in a bundle.
///
/// Currently stores offset. This may be removed in the future.
//
// due to a possible bug in the resource compiler Stingray uses there are some
// file types that can be stored with an incorrect file size
//
// a workaround is flagging files possibly affected by this and resolving the offets
// while extracting those files
//
// this is also done for older bundle formats since they do not store file size in the index
#[cfg_attr(feature = "serde_support", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct BundleFile {
    hash: u64,
    ext: u64,

    size: u32,

    /// May be removed in a future release.
    offset: u32,

    flags: u8,
}

impl BundleFile {
    pub fn new(hash: u64, ext: u64, size: u32, offset: u32) -> Self {
        Self {
            hash,
            ext,
            size,
            offset,
            flags: 0,
        }
    }

    pub fn name_hash(&self) -> u64 {
        self.hash
    }

    pub fn ext_hash(&self) -> u64 {
        self.ext
    }

    pub fn size(&self) -> u32 {
        self.size
    }

    pub fn offset(&self) -> u32 {
        self.offset
    }

    pub(crate) fn is_bad_offset(&self) -> bool {
        self.flags & FileFlags::BadOffset != 0
    }

    pub(crate) fn set_size(&mut self, size: u32) {
        self.size = size;
    }

    pub(crate) fn set_offset(&mut self, offset: u32) {
        self.offset = offset;
    }

    #[allow(dead_code)]
    pub(crate) fn kind(&self) -> u8 {
        if self.flags & FileFlags::Deleted2 != 0 {
            2
        } else if self.flags & FileFlags::Deleted != 0 {
            1
        } else {
            0
        }
    }

    pub(crate) fn set_kind(&mut self, kind: u32) {
        match kind {
            2 => self.flags |= FileFlags::Deleted2,
            1 => self.flags |= FileFlags::Deleted,
            _ => (),
        }
    }

    pub(crate) fn set_bad_offset(&mut self, set: bool) {
        if set {
            self.flags |= FileFlags::BadOffset;
        } else {
            self.flags &= !FileFlags::BadOffset;
        }
    }
}

pub fn get_file_interface<'a>(buffer: &'a [u8]) -> crate::StingrayResult<Box<dyn FileReader + 'a>> {
    let to_read: &[u8; 8] = buffer[0..8].try_into()?;
    let ext_hash = u64::from_le_bytes(*to_read);
    let to_read: &[u8; 8] = buffer[8..16].try_into()?;
    let _name_hash = u64::from_le_bytes(*to_read);

    let kind = FileKind::with_hash(ext_hash);

    let r: Box<dyn FileReader> = match kind {
        FileKind::lua => Box::new(lua::Lua::new(buffer)),
        FileKind::wwise_dep => Box::new(wwise_dep::WwiseDep::new(buffer)),
        FileKind::texture => Box::new(texture::Texture::new(buffer)),
        _ => Box::new(UnknownFile {buffer}),
    };
    Ok(r)
}

pub fn can_file_self_name(ext: u64) -> bool {
    matches!(FileKind::with_hash(ext),
        FileKind::lua
        | FileKind::wwise_dep)
}


