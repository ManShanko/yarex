use std::convert::TryInto;
use std::io::Write;

// DDS magic word
const MAGIC_WORD: u64 = u64::from_be(0x444453207c000000);

pub struct Texture<'a> {
    buffer: &'a [u8],
}

impl<'a> Texture<'a> {
    pub fn new(buffer: &'a [u8]) -> Self {
        Self {
            buffer,
        }
    }
}

impl<'a> super::FileReader<'a> for Texture<'a> {
    fn decompile(&mut self, out: &mut dyn Write) -> crate::StingrayResult<usize> {
        out.write_all(&self.buffer[36..])?;
        Ok(self.buffer[36..].len())
    }

    fn path(&self) -> (Option<&str>, Option<&str>) {
        if let Ok(array) = self.buffer[36..44].try_into() {
            let magic_word = u64::from_le_bytes(array);
            if magic_word == MAGIC_WORD {
                return (None, Some("dds"))
            }
        }
        (None, Some("texture"))
    }
}

