use std::convert::TryInto;
use std::io::Write;

// version? check
const VERSION: u32 = u32::from_be(0x05000000);

pub struct WwiseDep<'a> {
    buffer: &'a [u8],
}

impl<'a> WwiseDep<'a> {
    pub fn new(buffer: &'a [u8]) -> Self {
        Self {
            buffer,
        }
    }
}

impl<'a> super::FileReader<'a> for WwiseDep<'a> {
    fn decompile(&mut self, out: &mut dyn Write) -> crate::StingrayResult<usize> {
        out.write_all(&self.buffer[36..])?;
        Ok(self.buffer[36..].len())
    }

    fn path(&self) -> (Option<&str>, Option<&str>) {
        let name_hash = u64::from_le_bytes(self.buffer[8..16].try_into().unwrap());
        if let Ok(array) = self.buffer[36..40].try_into() {
            let version = u32::from_le_bytes(array);
            if version == VERSION {
                if let Ok(array) = self.buffer[40..44].try_into() {
                    let str_len = u32::from_le_bytes(array) as usize;
                    // read null terminated string
                    // str_len includes null at end so reduce by one
                    if let Ok(s) = std::str::from_utf8(&self.buffer[44..44 + str_len - 1]) {
                        if crate::hash::murmur_hash(s.as_bytes()) == name_hash {
                            return (Some(s), Some("wwise_dep"))
                        }
                    }
                }
            }
        }
        (None, Some("wwise_dep"))
    }
}

