use std::convert::TryInto;
use std::io::Write;

use crate::file;
use crate::file::Language;
use crate::file::FileReader;

pub struct Strings<'a> {
    buffer: &'a [u8],
}

impl<'a> Strings<'a> {
    pub fn new(buffer: &'a [u8]) -> Self {
        Self {
            buffer,
        }
    }
}

impl<'a> FileReader<'a> for Strings<'a> {
    fn decompile(&mut self, out: &mut dyn Write) -> crate::StingrayResult<usize> {
        let (info, mut offset) = file::get_file_info(self.buffer)?;
        let mut hashes = Vec::new();
        let mut buffer = Vec::with_capacity(0x10000);

        let mut first_lang = true;
        buffer.push(b'{');
        let variants = info.variants();
        for i in 0..variants.len() {
            let variant = &variants[i];
            hashes.clear();

            if !first_lang {
                buffer.push(b',');
            }
            first_lang = false;

            if let Some(lang) = variant.lang.as_str() {
                write!(buffer, "\"{}\":{{", lang)?;
            } else {
                match variant.lang {
                    Language::Unknown(x) => write!(buffer, "\"UNKNOWN_{}\":{{", x)?,
                    _ => unreachable!(),
                }
            }

            // 0..4 is unknown
            offset += 4;
            let num_strings = u32::from_le_bytes(self.buffer[offset..offset + 4].try_into()?);
            offset += 4;

            for _ in 0..num_strings {
                let hash = u32::from_le_bytes(self.buffer[offset..offset + 4].try_into()?);
                offset += 4;
                // 4..8 is string offset from start of variant
                offset += 4;

                hashes.push(hash);
            }

            let mut first_string = true;
            for hash in &hashes {
                if first_string {
                    write!(buffer, "\"{:08x}\":\"", hash)?;
                    first_string = false;
                } else {
                    write!(buffer, ",\"{:08x}\":\"", hash)?;
                }

                offset += file::copy_and_escape_cstr(&self.buffer[offset..], &mut buffer);

                buffer.push(b'"');
            }

            buffer.push(b'}');
        }
        buffer.push(b'}');

        debug_assert_eq!(offset, self.buffer.len());
        out.write_all(&buffer)?;

        Ok(buffer.len())
    }
}

