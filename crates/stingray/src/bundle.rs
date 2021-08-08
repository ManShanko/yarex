use std::convert::TryInto;
use std::io::{Read, Seek};

use crate::consts;
use crate::file::{BundleFile, FileKind};
use crate::utility::Patch;
use crate::utility::format_bundle;
use crate::reader::{BundleReader, ReadBuffer};

/// Convenience wrapper around [BundleVersion](BundleVersion).
///
/// Might be removed in a future update.
#[cfg_attr(feature = "serde_support", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct Bundle {
    /// Hash of resource package name/path.
    hash: u64,

    versions: Vec<BundleVersion>,
}

impl Bundle {
    pub fn new(hash: u64) -> Self {
        Bundle {
            hash,
            versions: Vec::new(),
        }
    }

    pub fn add_version(&mut self, version: BundleVersion) {
        if let Err(i) = self.versions.binary_search_by(|probe| probe.patch().cmp(&version.patch())) {
            self.versions.insert(i, version);
        }
    }

    pub fn remove_version(&mut self, patch: Patch) {
        if let Ok(i) = self.versions.binary_search_by(|probe| probe.patch().cmp(&patch)) {
            self.versions.remove(i);
        }
    }

    pub fn versions(&self) -> Vec<&BundleVersion> {
        self.versions.iter().collect()
    }

    pub fn versions_mut(&mut self) -> Vec<&mut BundleVersion> {
        self.versions.iter_mut().collect()
    }

    pub fn hash(&self) -> u64 {
        self.hash
    }

    pub fn files(&self) -> Vec<&BundleFile> {
        let mut out = Vec::new();
        for bundle in self.versions.iter().rev() {
            for file in &bundle.files {
                if file.size() > 0 {
                    out.push(file);
                }
            }
        }
        out.into_iter().filter(|file| file.size() > 0).collect()
    }

    pub fn active_files(&self) -> Vec<(u16, &BundleFile)> {
        let mut out = Vec::<(u16, &BundleFile)>::new();
        for bundle in self.versions.iter().rev() {
            for file in &bundle.files {
                if let Err(i) = out.binary_search_by(|(_, probe)| {
                    match probe.ext_hash().cmp(&file.ext_hash()) {
                        std::cmp::Ordering::Equal => probe.name_hash().cmp(&file.name_hash()),
                        x => x,
                    }
                }) {
                    out.insert(i, (bundle.patch, file));
                }
            }
        }

        let mut out = out.into_iter().filter(|(_, file)| file.size() > 0).collect::<Vec<_>>();
        out.sort_by(|a, b| match a.0.cmp(&b.0) {
            std::cmp::Ordering::Equal => a.1.offset().cmp(&b.1.offset()),
            x => x,
        });
        out
    }
}

/// Version/patch of a bundle.
#[cfg_attr(feature = "serde_support", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct BundleVersion {
    patch: u16,

    /// Difference in known size of files and actual size of files.
    diff: u32,

    /// Size of compressed bundle.
    size: u32,

    reader: BundleReader,
    files: Vec<BundleFile>,
}

impl BundleVersion {
    pub fn new(patch: u16, size: u64) -> Self {
        assert!(patch < 1000);
        assert!(size <= u32::MAX as u64);

        Self {
            patch,
            diff: 0,
            size: size as u32,
            reader: BundleReader::new(),
            files: Vec::new(),
        }
    }

    pub fn size(&self) -> u64 {
        self.size as u64
    }

    pub fn patch(&self) -> Patch {
        Patch::from(self.patch)
    }

    /// Get reference to `BundleReader` to enable optimizations for SSD/unbuffered IO.
    ///
    /// May be removed in future release.
    pub fn reader_mut(&mut self) -> &mut BundleReader {
        &mut self.reader
    }

    pub fn file(&self, ext_hash: u64, name_hash: u64) -> Option<&BundleFile> {
        match self.files.binary_search_by(|probe| (probe.ext_hash(), probe.name_hash()).cmp(&(ext_hash, name_hash))) {
            Ok(i) => self.files.get(i),
            Err(_) => None,
        }
    }

    fn get_file_index(&self, ext_hash: u64, name_hash: u64) -> Option<usize> {
        self.files.binary_search_by(|probe| {
            match probe.ext_hash().cmp(&ext_hash) {
                std::cmp::Ordering::Equal => probe.name_hash().cmp(&name_hash),
                x => x,
            }
        }).ok()
    }

    pub fn files(&self) -> Vec<&BundleFile> {
        self.files.iter().filter(|file| file.size() > 0).collect()
    }

    /// Read bundle header data at `4..260`.
    pub fn header<'a>(
        &mut self,
        fd: &mut (impl Read + Seek),
        buffer: &'a mut ReadBuffer,
    ) -> crate::StingrayResult<&'a [u8]> {
        self.reader.read(fd, buffer, 4..260, None)
    }

    /// Read bundle index.
    pub fn index(
        &mut self,
        fd: &mut (impl Read + Seek),
        bundle_hash: u64,
        buffer: &mut ReadBuffer,
    ) -> crate::StingrayResult<u64> {
        let mut read = 0;
        let mut read_raw = 0;

        #[cfg(debug_assertions)]
        let bundle_name = format_bundle(bundle_hash, self.patch);

        let scrap = self.reader.read(fd, buffer, 0..260, Some(&mut read_raw))?;
        read += read_raw;
        let format = self.reader.version().ok_or_else(|| stingray_error!("no format for bundle"))?;
        let index_size = if format < 6 {
            20
        } else {
            24
        };

        let num_files = u32::from_le_bytes(scrap[0..4].try_into()?) as usize;

        let t: usize = num_files as usize * index_size;
        //if t > scrap.len() {
        //    scrap.resize(t, 0);
        //}
        let scrap = self.reader.read(fd, buffer, 260..260 + t, Some(&mut read_raw))?;
        read += read_raw;

        let uncompressed_size = self.reader.size();

        let mut offset = 260 + t as u64;
        let mut test = 0;
        self.files.truncate(0);
        self.files.reserve(num_files);
        for i in 0..num_files {
            let b = i * index_size;
            let ext = u64::from_le_bytes(scrap[b..b + 8].try_into()?);
            let name = u64::from_le_bytes(scrap[b + 8..b + 16].try_into()?);
            let kind = u32::from_le_bytes(scrap[b + 16..b + 20].try_into()?);

            if offset > u32::MAX as u64 {
                return Err(stingray_error!(
                    "bundle is bigger than expected {}-{}: {} >= {}, {}, {}",
                    uncompressed_size, num_files, offset, u32::MAX, test, b));
            }
            if offset > uncompressed_size {
                return Err(stingray_error!(
                    "offset is larger than bundle size {} > {} (p: {}, s: {}, f: {}, t: {}, i: {}, b: {})",
                    offset, uncompressed_size, self.patch, uncompressed_size, num_files, test, i, b));
            }

            // only format 5 and 6 have been tested
            let size = if format < 6 {
                0
            } else {
                u32::from_le_bytes(scrap[b + 20..b + 24].try_into()?)
            };

            #[cfg(debug_assertions)]
            if let FileKind::Unknown = FileKind::with_hash(ext) {
                println!("bundle \"{}\" has invalid hash in index at offset {}", bundle_name, 260 + b);
            }

            test = size;
            let file_offset = offset;
            offset += if size > 0 {
                36 + size as u64
            } else if kind == 1 || kind == 2 {
                24
            } else { 0 };

            let mut file = BundleFile::new(name, ext, size, file_offset as u32);
            file.set_kind(kind);
            self.files.push(file);
        }

        // check if bundle is larger than the sum of file sizes from the index
        // if so then invalidate all files that might be incorrectly size
        if offset != uncompressed_size {
            let (diff, overflow) = uncompressed_size.overflowing_sub(offset);
            if overflow {
                return Err(stingray_error!("diff underflow"));
            }
            self.diff = diff as u32;

            // legacy bundle formats do not store size in index
            if format < 6 {
                for file in &mut self.files {
                    file.set_bad_offset(true);
                }
            } else {
                let mut extensions = Vec::new();

                let mut last = None;
                let mut start = None;
                let mut end = None;
                for (i, file) in self.files.iter_mut().enumerate() {
                    let kind = FileKind::with_hash(file.ext_hash());

                    match kind {
                        // assume unknown files are incorrectly sized for
                        // maximum capability with unsupported file types
                        FileKind::Unknown
                        | FileKind::particles
                        | FileKind::slug
                        | FileKind::strings => {
                            if last.is_none() || last != Some(kind) {
                                last = Some(kind);
                                extensions.push((file.ext_hash(), kind.as_str().map(|s| s.to_owned())));
                            }

                            if start.is_none() {
                                start = Some(i);
                            }

                            end = Some(i + 1);
                        },
                        _ => (),
                    }
                }

                // invalidate size of any file between the first and last
                // detected file
                if let Some(start) = start {
                    if let Some(end) = end {
                        if start + 1 == end {
                            let file = &mut self.files[start];
                            file.set_size(file.size() + diff as u32);
                        } else {
                            for file in &mut self.files[start..end] {
                                file.set_bad_offset(true);
                            }
                        }

                        for file in &mut self.files[end..num_files] {
                            file.set_offset(file.offset() + diff as u32);
                        }
                    } else {
                        return Err(stingray_error!(
                            "fallthrough2 {} {} {}",
                            format_bundle(bundle_hash, self.patch),
                            diff,
                            self.files[start].offset()));
                    }
                } else {
                    return Err(stingray_error!(
                        "fallthrough1 {} {}",
                        format_bundle(bundle_hash, self.patch),
                        diff));
                }
            }
        }

        self.files.sort_by(|a, b| {
            (a.ext_hash(), a.name_hash()).cmp(&(b.ext_hash(), b.name_hash()))
        });

        Ok(read as u64)
    }

    /// Read a file from this `BundleVersion`.
    pub fn read_file<'a>(
        &mut self,
        fd: &mut (impl Read + Seek),
        bundle_hash: u64,
        ext_hash: u64,
        file_hash: u64,
        buffer: &'a mut ReadBuffer,
    ) -> crate::StingrayResult<&'a [u8]> {
        let diff = self.diff as usize;

        let i = self.get_file_index(ext_hash, file_hash)
            .ok_or_else(|| stingray_error!("failed to get file"))?;
        let (left, right) = self.files.split_at_mut(i + 1);
        let file = left.get_mut(i)
            .ok_or_else(|| stingray_error!("failed to get file"))?;
        let file_offset = file.offset() as usize;
        let reader = &mut self.reader;

        let out = if diff > 0 {
            if file.is_bad_offset() {
                let next_file = right.get_mut(0);
                let target = (file.ext_hash().to_le_bytes(), file.name_hash().to_le_bytes());

                // TODO do chunking when diff is large to minimize buffer size
                let mut size = consts::FILE_HEADER_SIZE + file.size() as usize + diff + 16;

                let mut max_size = if let Some(ref f) = next_file {
                    if !f.is_bad_offset() {
                        f.offset() as usize + 16
                    } else {
                        reader.size() as usize
                    }
                } else {
                    reader.size() as usize
                };

                max_size -= file.offset() as usize;
                if size > max_size {
                    size = max_size;
                }

                let scrap = reader.read(fd, buffer, file_offset..file_offset + size, None)?;

                let mut start = 0;
                while start < size - 16 {
                    if scrap[start..start + 8] == target.0
                        && scrap[start + 8..start + 16] == target.1
                    {
                        break;
                    }
                    start += 1;
                }

                let end = if let Some(next_file) = next_file {
                    let target = (next_file.ext_hash().to_le_bytes(), next_file.name_hash().to_le_bytes());
                    let mut offset = size - 16;
                    loop {
                        if scrap[offset..offset + 8] == target.0
                            && scrap[offset + 8..offset + 16] == target.1
                        {
                            break;
                        }
                        if offset == 0 {
                            return Err(stingray_error!(
                                    "underflow in bundle {} for files {:016x} {:016x} and {:016x} {:016x}",
                                    format_bundle(bundle_hash, self.patch),
                                    file.ext_hash().swap_bytes(),
                                    file.name_hash().swap_bytes(),
                                    next_file.ext_hash().swap_bytes(),
                                    next_file.name_hash().swap_bytes()
                                ));
                        }
                        offset -= 1;
                    }
                    offset
                } else { size - 16 };

                file.set_offset(file.offset() + start as u32);
                file.set_size((end - start) as u32);
                file.set_bad_offset(false);

                &scrap[start..end]
            } else {
                let size = consts::FILE_HEADER_SIZE + file.size() as usize;

                reader.read(fd, buffer, file_offset..file_offset + size, None)?
            }
        } else {
            if file.is_bad_offset() {
                return Err(stingray_error!("diff is 0 but file has bad offset"));
            }

            let size = consts::FILE_HEADER_SIZE + file.size() as usize;

            if (file.offset() as u64 + size as u64) > reader.size() {
                return Err(stingray_error!(
                    "file offset ({}) and size ({}) is bigger than the raw bundle size ({})",
                    file.offset(), size, reader.size()));
            }

            reader.read(fd, buffer, file_offset..file_offset + size, None)?
        };

        if out.len() < 36 {
            return Err(stingray_error!(
                "error processing file {:016x} {:016x} in bundle \"{}\"",
                file.ext_hash().swap_bytes(),
                file.name_hash().swap_bytes(),
                format_bundle(bundle_hash, self.patch)));
        }

        let ext_hash = u64::from_le_bytes(out[..8].try_into()?);
        let name_hash = u64::from_le_bytes(out[8..16].try_into()?);

        assert!(ext_hash == file.ext_hash() && name_hash == file.name_hash(),
            "bundle \"{}\" has hash mismatch {:016x} != {:016x} ({:?}) || {:016x} != {:016x} at offset {}",
            format_bundle(bundle_hash, self.patch),
            ext_hash.swap_bytes(),
            file.ext_hash().swap_bytes(),
            FileKind::with_hash(file.ext_hash()).as_str(),
            name_hash.swap_bytes(),
            file.name_hash().swap_bytes(),
            file.offset());

        Ok(out)
    }
}
