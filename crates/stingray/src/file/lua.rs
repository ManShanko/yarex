use std::convert::TryInto;
use std::io::Write;

// LuaJIT magic word
const MAGIC_WORD: u32 = u32::from_be(0x1b4c4a02);

pub struct Lua<'a> {
    buffer: &'a [u8],
}

impl<'a> Lua<'a> {
    pub fn new(buffer: &'a [u8]) -> Self {
        Self {
            buffer,
        }
    }
}

impl<'a> super::FileReader<'a> for Lua<'a> {
    fn decompile(&mut self, out: &mut dyn Write) -> crate::StingrayResult<usize> {
        // lua bytecode in Stingray has 12 bytes of prepended data that we skip
        let magic_word = u32::from_le_bytes(self.buffer[48..52].try_into()?);
        if magic_word != MAGIC_WORD {
            return Err(stingray_error!(
                "lua magic word expected {:08x} but got {:08x}", MAGIC_WORD, magic_word));
        }

        out.write_all(&self.buffer[48..])?;
        Ok(self.buffer[48..].len())
    }

    fn path(&self) -> (Option<&str>, Option<&str>) {
        let str_len = self.buffer[53] as usize;

        // 55 to skip @ character that is included in string
        // - 5 to shrink for @ character and ".lua"
        if let Ok(path) = std::str::from_utf8(&self.buffer[55..55 + str_len - 5]) {
            return (Some(path), Some("lua"));
        }

        (None, None)
    }
}

