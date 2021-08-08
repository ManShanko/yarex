// TODO refactor

//! Segment reader for the `bundle` package format.
use std::convert::TryInto;
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicU32, Ordering};
use std::ops::Range;

use flate2::read::ZlibDecoder;
use super::consts::ZLIB_CHUNK_SIZE;

/// Unbuffered IO aligned read size.
const ALIGNED_READ_SIZE: usize = 4096;

/// Size of header for compressed bundles.
const BUNDLE_COMPRESSED_HEADER_SIZE: usize = 12;

#[doc(hidden)]
static BUNDLE_READER_ID: AtomicU32 = AtomicU32::new(1);

// Helper function for decompressing from bytes to out.
#[doc(hidden)]
fn read_compressed(bytes: &[u8], out: &mut [u8]) -> io::Result<usize> {
    let mut z = ZlibDecoder::new(bytes);
    z.read_exact(out)?;
    Ok(out.len())
}

/// Pool object for caching reads and reducing allocations.
///
/// Used internally by the bundle reader object.
pub struct ReadBuffer {
    src: Vec<u8>,
    out: Vec<u8>,
    id: Option<u32>,
    offset: u64,
    last: u64,
    start: Option<usize>,
    end: Option<usize>,
}

impl ReadBuffer {
    pub const CHUNK_SIZE: usize = ZLIB_CHUNK_SIZE;

    #[doc(hidden)]
    fn new_(size: usize) -> Self {
        Self {
            src: vec![0; size],
            out: vec![0; ZLIB_CHUNK_SIZE],
            id: None,
            offset: 0,
            last: 0,
            start: None,
            end: None,
        }
    }

    /// Creates `ReadBuffer` with a custom buffer size.
    ///
    /// If unbuffered IO is enabled then the buffer may increased in size to pad for aligned reads.
    pub fn new(size: usize) -> Self {
        Self::new_(size)
    }

    fn pad(&mut self) {
        match self.src.len() % ALIGNED_READ_SIZE {
            0 => (),
            x => self.src.resize(self.src.len() + ALIGNED_READ_SIZE - x, 0),
        }
    }

    fn reset(&mut self) {
        self.offset = 0;
        self.last = 0;
        self.start = None;
        self.end = None;
    }
}

impl Default for ReadBuffer {
    fn default() -> Self {
        Self::new_(ZLIB_CHUNK_SIZE + ALIGNED_READ_SIZE * 2)
    }
}

/// Get incrementing unique ID from an atomic to use with [BundleReader](BundleReader) for [ReadBuffer](ReadBuffer)s.
fn get_unique_id() -> u32 {
    BUNDLE_READER_ID.fetch_add(1, Ordering::SeqCst)
}

/// Reader for the bundle format.
///
/// It handles seeking over compressed bundles to read data at uncompressed offsets.
/// Internally it uses caching through [`ReadBuffer`](ReadBuffer) that is supplied by the user.
///
/// Compressed chunks are prefixed with a `u32` size that is always `65536` or smaller.
/// If the chunk size is `65536` than that chunk is not compressed.
/// Otherwise, the chunk is compressed with zlib deflate.
///
/// Uncompressed a chunk is always `65536` bytes. Chunks at EOF are 0 padded to `65536`.
#[cfg_attr(feature = "serde_support", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct BundleReader {
    #[cfg_attr(feature = "serde_support", serde(skip))]
    offset: u32,
    size: u32,

    // zero means no compression
    // no compression means size is ZLIB_CHUNK_SIZE
    chunk_sizes: Vec<u16>,

    #[cfg_attr(feature = "serde_support", serde(skip, default = "get_unique_id"))]
    id: u32,
    #[cfg_attr(feature = "serde_support", serde(skip))]
    version: Option<u16>,
    #[cfg_attr(feature = "serde_support", serde(skip))]
    is_ssd: bool,
    #[cfg_attr(feature = "serde_support", serde(skip))]
    unbuffered: bool,
}

impl BundleReader {
    pub fn new() -> Self {
        Self {
            offset: 0,
            size: 0,
            chunk_sizes: Vec::new(),
            id: get_unique_id(),
            version: None,
            is_ssd: false,
            unbuffered: false,
        }
    }

    /// Size of uncompressed bundle.
    pub fn size(&self) -> u64 {
        self.size as u64
    }

    /// `6` is the bundle version used in Vermintide 2.
    ///
    /// `5` is the bundle version used in Vermintide 2 mods and older games.
    pub fn version(&self) -> Option<u16> {
        self.version
    }

    /// Disable HDD performance optimizations to accelerate reads for SSDs.
    pub fn ssd_accelerator(&mut self, enable: bool) {
        self.is_ssd = enable;
    }

    /// Turn on aligned reads for unbuffered IO.
    ///
    /// Only used on Windows.
    pub fn unbuffered(&mut self, enable: bool) {
        self.unbuffered = enable;
    }

    /// Reads `range` of the uncompressed bundle.
    pub fn read<'a>(
        &mut self,
        fd: &mut (impl Read + Seek),
        read_buffer: &'a mut ReadBuffer,
        range: Range<usize>,
        read_raw: Option<&mut u64>,
    ) -> crate::StingrayResult<&'a [u8]> {
        if self.unbuffered {
            read_buffer.pad();
        }

        let mut ret: u64 = 0;
        let mut read: usize = 0;
        let to_read = range.end - range.start;
        let mut chunk = range.start / ZLIB_CHUNK_SIZE;
        let mut chunk_offset = range.start % ZLIB_CHUNK_SIZE;

        if read_buffer.id != Some(self.id) {
            read_buffer.reset();
            read_buffer.id = Some(self.id);
        }

        if read_buffer.out.len() < to_read + chunk_offset {
            let diff = to_read + chunk_offset + ZLIB_CHUNK_SIZE + read_buffer.out.len();
            let chunks = (diff - read_buffer.out.len()) / ZLIB_CHUNK_SIZE;
            read_buffer.out.resize(chunks * ZLIB_CHUNK_SIZE, 0);
        }

        let mut count = 0;
        while read < to_read as usize {
            let mut size: usize = if ZLIB_CHUNK_SIZE as u64 > (chunk_offset + to_read - read) as u64 {
                to_read - read
            } else {
                ZLIB_CHUNK_SIZE - chunk_offset as usize
            };

            if size == 0 {
                return Err(stingray_error!("zlib segment has size 0"));
            } else if size > ZLIB_CHUNK_SIZE {
                return Err(stingray_error!("zlib segment has size greater than expected"));
            }

            if to_read < (read + size) as usize {
                dbg!(to_read, read, size, range, chunk_offset);
                return Err(stingray_error!(
                    "something went wrong, amount to read is bigger than remainder left"));
            }

            if count == 0 && read_buffer.end == Some(chunk) {
                if read_buffer.start != read_buffer.end {
                    let start = read_buffer.start.unwrap();
                    let end = read_buffer.end.unwrap();
                    let off = (end - start - 1) * ZLIB_CHUNK_SIZE;

                    let (left, right) = read_buffer.out.split_at_mut(ZLIB_CHUNK_SIZE);
                    left.copy_from_slice(&right[off..off + ZLIB_CHUNK_SIZE]);
                }
            } else if chunk_offset > 0 {
                let len: usize = if to_read > (ZLIB_CHUNK_SIZE - chunk_offset as usize) {
                    ZLIB_CHUNK_SIZE - chunk_offset
                } else {
                    to_read
                };

                ret += self.read_chunk(
                    fd,
                    chunk,
                    &mut read_buffer.last,
                    &mut read_buffer.offset,
                    &mut read_buffer.src[..],
                    &mut read_buffer.out[count * ZLIB_CHUNK_SIZE..(count + 1) * ZLIB_CHUNK_SIZE],
                    true)?;

                size = len;
            } else {
                if to_read < (read + size) {
                    return Err(stingray_error!(
                        "something went wrong, amount to read is bigger than remainder left"));
                }

                ret += self.read_chunk(
                    fd,
                    chunk,
                    &mut read_buffer.last,
                    &mut read_buffer.offset,
                    &mut read_buffer.src[..],
                    &mut read_buffer.out[count * ZLIB_CHUNK_SIZE..(count + 1) * ZLIB_CHUNK_SIZE],
                    true)?;
            }
            read += size;
            chunk_offset = 0;
            chunk += 1;
            count += 1;

            //if self.size != 0 && read == self.size as usize {
            //    to_read = self.size as usize;
            //}
        }

        if let Some(read_raw) = read_raw {
            *read_raw = ret;
        }

        let start = range.start;
        let end = range.start + to_read;

        read_buffer.start = Some(start / ZLIB_CHUNK_SIZE);
        read_buffer.end = match end % ZLIB_CHUNK_SIZE {
            0 => Some((end / ZLIB_CHUNK_SIZE).saturating_sub(1)),
            _ => Some(end / ZLIB_CHUNK_SIZE),
        };

        let chunk_offset = start % ZLIB_CHUNK_SIZE;
        Ok(&read_buffer.out[chunk_offset..chunk_offset + to_read])
    }

    #[doc(hidden)]
    fn get_offset(&self, chunk: usize) -> u64 {
        let mut out = if chunk == 0 { 0 } else {
            BUNDLE_COMPRESSED_HEADER_SIZE as u64
        };

        for size in &self.chunk_sizes[..chunk] {
            let size = *size;
            out += 4 + if size == 0 {
                ZLIB_CHUNK_SIZE as u64
            } else { size as u64 };
        }
        out
    }

    #[doc(hidden)]
    fn read_chunk(
        &mut self,
        fd: &mut (impl Read + Seek),
        chunk: usize,
        last: &mut u64,
        offset: &mut u64,
        source: &mut [u8],
        out: &mut [u8],
        use_buffer: bool
    ) -> crate::StingrayResult<u64> {
        let mut ret = 0;
        let mut co_len = self.chunk_sizes.len();
        if co_len < chunk {
            for i in co_len..chunk {
                ret += self.read_chunk(fd, i, last, offset, source, out, false)?;
            }
            co_len = self.chunk_sizes.len();
        }

        let co = self.get_offset(chunk);

        let off = match chunk {
            0 => BUNDLE_COMPRESSED_HEADER_SIZE,
            _ => 0,
        };

        let mut size = if use_buffer {
            // Optimization for long lived readers
            if chunk + 1 < co_len {
                (self.get_offset(chunk + 1) - co) as usize
            //} else if self.is_ssd {
            //    ZLIB_CHUNK_SIZE + 8
            } else {
                source.len()
            }
        } else if self.is_ssd {
            match chunk {
                0 => 16,
                _ => 4,
            }
        } else {
            source.len()
        };

        if co == 0 && chunk != 0 {
            return Err(stingray_error!("offset is 0 but chunk is not ({})", chunk));
        }

        // last is always greater than 0 in active chunk
        let read = if *last == 0
            || co + ZLIB_CHUNK_SIZE as u64 + 8 >= *offset + *last

            // BundleFile.is_bad_offset() can mess with offsets
            // if that happens then force new read
            || *offset > co as u64
        {
            // align chunk offset for unbuffered reads
            let seek_to = if self.unbuffered {
                if source.len() < ZLIB_CHUNK_SIZE + ALIGNED_READ_SIZE {
                    panic!("unbuffered reads need a larger buffer");
                } else if source.len() % ALIGNED_READ_SIZE > 0 {
                    panic!("unbuffered reads need an aligned buffer");
                }

                size += co as usize % ALIGNED_READ_SIZE;
                size = if size % ALIGNED_READ_SIZE > 0 {
                    size + (ALIGNED_READ_SIZE - size % ALIGNED_READ_SIZE)
                } else { size };

                if size > source.len() {
                    let len = source.len();
                    size = len - len % ALIGNED_READ_SIZE;
                }

                co - co % ALIGNED_READ_SIZE as u64
            } else { co };

            *offset = seek_to;

            fd.seek(SeekFrom::Start(seek_to as u64)).unwrap_or_else(|_| panic!("seek fail"));
            let read = fd.read(&mut source[0..size]).unwrap_or_else(|_| panic!("read fail {}", size)) as u64;
            *last = read as u64;
            ret += read;
            read
        } else {
            0
        };

        if *offset > co {
            return Err(stingray_error!(
                "chunk offset is invalid {}, {}, {}, {}, {}, {}",
                read, size, co, off, *offset, *last));
        }

        let start = (co - *offset) as usize;
        let end = if read > 0 {
            read
        } else {
            *last
        } as usize;
        let source = &source[start..end];
        let read = (end - start) as u64;

        if chunk == 0 {
            self.version = Some(u16::from_le_bytes(source[..2].try_into()?));
            self.size = u32::from_le_bytes(source[4..8].try_into()?);
        }
        let len = u32::from_le_bytes(source[off..off + 4].try_into()?);

        if len as usize > ZLIB_CHUNK_SIZE {
            return Err(stingray_error!(
                "bundle segment has invalid length {} > {} ({}, {}, {}, {}, {}, {}) {:?}",
                len, ZLIB_CHUNK_SIZE, ret, co, off, size, self.size, chunk, self.chunk_sizes));
        }

        if use_buffer {
            if out.len() != ZLIB_CHUNK_SIZE {
                return Err(stingray_error!("output buffer is smaller then ZLIB_CHUNK_SIZE"));
            }
            match len {
                x if x < 0x10000 => { read_compressed(&source[off + 4..off + 4 + len as usize], out)?; },
                65536 => out.copy_from_slice(&source[off + 4..off + 4 + ZLIB_CHUNK_SIZE]),
                _ => panic!(),
            }
        }

        let mut chunk_count = chunk;
        let mut offset = off;
        while read >= (offset + 4) as u64 {
            let len =  u32::from_le_bytes(source[offset..offset + 4].try_into()?);
            if len as usize > ZLIB_CHUNK_SIZE {
                return Err(stingray_error!(
                    "bundle segment has invalid length {} > {} ({}, {}, {}, {}, {}, {}) {:?}",
                    len, ZLIB_CHUNK_SIZE, ret, co, off, size, self.size, chunk, self.chunk_sizes));
            }

            if chunk_count >= co_len {
                let size = if len == ZLIB_CHUNK_SIZE as u32 {
                    0
                } else {
                    len
                } as u16;
                self.chunk_sizes.push(size);
            }

            chunk_count += 1;
            offset += 4 + len as usize;
        }

        Ok(ret)
    }
}

impl Default for BundleReader {
    fn default() -> Self {
        Self::new()
    }
}
