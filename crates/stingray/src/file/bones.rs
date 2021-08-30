use std::convert::TryInto;
use std::io::Write;

use crate::file;
use crate::file::FileReader;

pub struct Bones<'a> {
    buffer: &'a [u8],
}

impl<'a> Bones<'a> {
    pub fn new(buffer: &'a [u8]) -> Self {
        Self {
            buffer,
        }
    }
}

impl<'a> FileReader<'a> for Bones<'a> {
    fn decompile(&mut self, out: &mut dyn Write) -> crate::StingrayResult<usize> {
        let (_, mut offset) = file::get_file_info(self.buffer)?;

        let num_bones = u32::from_le_bytes(self.buffer[offset..offset + 4].try_into()?) as usize;
        offset += 4;
        let num_lods = u32::from_le_bytes(self.buffer[offset..offset + 4].try_into()?) as usize;
        offset += 4;

        // skip bone hashes (32bit)
        offset += num_bones * 4;

        let mut lods = Vec::with_capacity(num_lods);
        for _ in 0..num_lods {
            lods.push(u32::from_le_bytes(self.buffer[offset..offset + 4].try_into()?));
            offset += 4;
        }

        let mut buffer = Vec::with_capacity(0x2000);
        buffer.extend(b"{\"bones\":[");

        let mut first_bone = true;
        for _ in 0..num_bones {
            if !first_bone {
                buffer.push(b',');
            }
            first_bone = false;

            buffer.push(b'"');
            offset += file::copy_and_escape_cstr(&self.buffer[offset..], &mut buffer);
            buffer.push(b'"');
        }

        buffer.extend(b"],\"lod_levels\":[");

        let mut first_lod = true;
        for lod in &lods {
            if first_lod {
                write!(buffer, "{}", lod)?;
                first_lod = false;
            } else {
                write!(buffer, ",{}", lod)?;
            }
        }

        buffer.extend(b"]}");

        debug_assert_eq!(offset, self.buffer.len());
        out.write_all(&buffer)?;

        Ok(buffer.len())
    }
}

