use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::io::{self, BufRead, Write};
use std::fmt::Write as OtherWrite;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Instant, Duration, SystemTime};
use std::sync::mpsc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use stingray::{Bundle, BundleVersion, BundleFile, BundleReader, ReadBuffer, Patch};
use stingray::format_bundle;
use stingray::file::{FileKind, get_file_interface, can_file_self_name};
use stingray::hash::{self, KeyMap};

mod files;
use files::scan_dir_filter;
pub use files::Reader as Reader;

use super::utility::{
    size_to_string,
    load_reader,
    //format_bundle,
};

#[allow(dead_code)]
fn calculate_time_hash(time: SystemTime) -> u64 {
    let mut s = DefaultHasher::new();
    time.hash(&mut s);
    s.finish()
}

fn hash_bundle_database(dir: &Path) -> u64 {
    let db = dir.join("bundle_database.data");
    match std::fs::read(db) {
        Ok(bytes) => {
            let mut s = DefaultHasher::new();
            bytes.hash(&mut s);
            s.finish()
        }
        Err(_) => 0,
    }
}

pub enum IndexEvent {
    Size(u32),
    Progress {
        read: u64,
        count: u32,
    },
    End,
}

pub fn load_index(
    dir: &Path,
    index_file: Option<&Path>,
    num_threads: usize,
    benchmark: bool,
) -> Result<Index, Box<dyn std::error::Error>> {
    let mut index = if let Some(index_file) = index_file {
        if let Ok(index) = load_reader(index_file) {
            if !index.has_updated() {
                println!("Using {}", index_file.display());
                return Ok(index);
            } else {
                println!("Updating {}", index_file.display());
                index
            }
        } else {
            println!("Creating new index");
            Index::new(dir)
        }
    } else {
        println!("Force creating new index");
        Index::new(dir)
    };

    index.index_files_with_progress(num_threads, benchmark)?;

    Ok(index)
}

#[cfg_attr(feature = "serde_support", derive(serde::Serialize, serde::Deserialize))]
pub struct Index {
    dir: PathBuf,
    is_ssd: bool,
    hash: u64,

    bundles: Vec<Bundle>,

    timestamps: HashMap<(u64, Patch), u64>,

    #[cfg_attr(feature = "serde_support", serde(skip))]
    dirty: bool,

    #[cfg_attr(feature = "serde_support", serde(skip))]
    key_map: KeyMap,
}

impl Index {
    fn new(dir: &Path) -> Self {
        Self {
            dir: dir.to_owned(),
            is_ssd: false,
            hash: hash_bundle_database(dir),
            bundles: Vec::new(),
            timestamps: HashMap::new(),
            dirty: false,
            key_map: KeyMap::default(),
        }
    }

    pub fn has_updated(&self) -> bool {
        self.hash != hash_bundle_database(&self.dir)
    }

    #[allow(dead_code)]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn dirty(&self) -> bool {
        self.dirty
    }

    pub fn load_keys(&mut self, path: &Path) {
        if let Ok(fd) = File::open(path) {
            let reader = io::BufReader::new(fd).lines();
            for line in reader.flatten() {
                self.key_map.add_key(&line);
            }
        }
    }

    pub fn get_all_versions(&self) -> Vec<&BundleVersion> {
        let mut out = Vec::new();
        for bundle in &self.bundles {
            out.extend(bundle.versions());
        }
        out
    }

    pub fn get_all_files(&self) -> Vec<&BundleFile> {
        let mut out = Vec::new();
        for bundle in &self.bundles {
            out.extend(bundle.files());
        }
        out
    }

    pub fn get_unique_files(&self) -> Vec<&BundleFile> {
        let mut out = HashMap::<(u64, u64, Patch), &BundleFile>::with_capacity(1024*256);
        for bundle in &self.bundles {
            for version in &bundle.versions() {
                let patch = version.patch();
                for file in version.files() {
                    out.entry((file.name_hash(), file.ext_hash(), patch)).or_insert(file);
                }
            }
        }
        out.into_iter().map(|(_, file)| file).collect()
    }

    pub fn get_active_files(&self) -> Vec<&BundleFile> {
        let mut out = HashMap::<(u64, u64), &BundleFile>::with_capacity(1024*256);
        for bundle in &self.bundles {
            for (_, file) in bundle.active_files() {
                out.entry((file.name_hash(), file.ext_hash())).or_insert(file);
            }
        }
        out.into_iter().map(|(_, file)| file).collect()
    }

    pub fn index_files_with_progress(&mut self, num_threads: usize, unbuffered: bool) -> Result<(), Box<dyn std::error::Error>> {
        self.dirty = true;
        let (tx, rx) = mpsc::channel();

        let start = Instant::now();
        println!();
        println!("Starting index...");
        let t = thread::spawn(move || -> io::Result<()> {
            let (read, count, _size) = load_bar(rx)?;

            let millis = start.elapsed().as_millis();
            println!(
                "Read {} and indexed {} bundles in {}.{:02} seconds",
                size_to_string(read),
                count,
                millis / 1000,
                (millis % 1000) / 10
            );

            Ok(())
        });

        self.index_files_mt(num_threads, unbuffered, Some(tx.clone()))?; //blocking call

        tx.send(IndexEvent::End).unwrap();

        t.join().unwrap()?;

        Ok(())
    }

    fn index_files_mt(
        &mut self,
        num_threads: usize,
        unbuffered: bool,
        send: Option<mpsc::Sender<IndexEvent>>
    ) -> Result<(), Box<dyn std::error::Error>> {
        let files = self.find_and_check_bundles();
        if files.is_empty() {
            return Ok(());
        }

        let dir = &self.dir;
        let bundles = &Mutex::new(&mut self.bundles);
        let count = &AtomicU32::new(0);
        let reader = &Reader::new(false);

        crossbeam_utils::thread::scope(|s| {
            reader.open_bundles(s, dir, files, num_threads, unbuffered);

            let mut threads = Vec::with_capacity(num_threads);
            for _ in 0..num_threads {
                let send = send.as_ref().cloned();
                threads.push(s.spawn(move |_| {
                    let mut read_buffer = ReadBuffer::default();
                    while let Some((mut file, hash, patch, is_ssd)) = reader.pop() {
                        let mut version = BundleVersion::new(patch, file.metadata().unwrap().len());

                        let reader = version.reader_mut();
                        reader.ssd_accelerator(is_ssd);

                        #[cfg(target_os = "windows")]
                        reader.unbuffered(unbuffered);

                        let read = version.index(&mut file, hash, &mut read_buffer)
                            .unwrap_or_else(|e| panic!("bundle \"{}\" failed index with error: {}",
                                format_bundle(hash, patch),
                                e));
                        {
                            let mut bundles = bundles.lock().unwrap();
                            match bundles.binary_search_by(|probe| probe.hash().cmp(&hash)) {
                                Ok(i) => bundles.get_mut(i).unwrap().add_version(version),
                                Err(i) => {
                                    let mut bundle = Bundle::new(hash);
                                    bundle.add_version(version);
                                    bundles.insert(i, bundle);
                                }
                            }
                        }

                        if let Some(ref send) = send {
                            send.send(IndexEvent::Progress {
                                read,
                                count: 1 + count.fetch_add(1, Ordering::SeqCst),
                            }).unwrap();
                        }
                    }
                }));
            }

            if let Some(ref send) = send {
                send.send(IndexEvent::Size(reader.num_files() as u32)).unwrap();
            }

            for thread in threads {
                thread.join().unwrap();
            }
        }).unwrap();

        self.is_ssd = reader.is_ssd();

        Ok(())
    }

    pub fn extract_files_with_progress(
        &mut self,
        out_dir: Option<&Path>,
        pattern: &str,
        num_threads: usize,
        unbuffered: bool,
        hash_fallback: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.dirty = true;
        let (tx, rx) = mpsc::channel();

        let start = Instant::now();
        println!();
        match out_dir {
            Some(p) => println!("Extracting to {}...", p.display()),
            None => println!("[DEBUG] Extracting to buffer in ram..."),
        }
        let t = thread::spawn(move || -> io::Result<()> {
            let (read, count, _size) = load_bar(rx)?;

            let millis = start.elapsed().as_millis();
            println!(
                "Extracted {} files ({}) in {}.{:02} seconds",
                count,
                size_to_string(read),
                millis / 1000,
                (millis % 1000) / 10
            );

            Ok(())
        });

        self.extract_files_mt(out_dir, pattern, num_threads, unbuffered, hash_fallback, Some(tx.clone()))?;

        tx.send(IndexEvent::End).unwrap();

        t.join().unwrap()?;

        Ok(())
    }

    fn extract_files_mt(
        &mut self,
        out_dir: Option<&Path>,
        pattern: &str,
        mut num_threads: usize,
        unbuffered: bool,
        hash_fallback: bool,
        send: Option<mpsc::Sender<IndexEvent>>
    ) -> Result<(), Box<dyn std::error::Error>> {
        if !self.is_ssd {
            num_threads = 1;
        }

        let mut split: Vec<&str> = pattern.split('.').collect();

        // assume wildcard if no extension is passed
        let last = if split.len() > 1 {
            split.pop().unwrap()
        } else {
            "*"
        };

        let extension_hash = FileKind::with_str(last) as u64;
        let filter_extension = |extension: u64| -> bool {
            match last {
                "*" => true,
                _ => extension == extension_hash,
            }
        };

        let path = if split.is_empty() {
            last.to_string()
        } else {
            split.into_iter().fold(String::new(), |mut a, b| {
                a.reserve(b.len() + 1);
                if !a.is_empty() {
                    a.push('.');
                }
                a.push_str(b);
                a
            })
        };
        self.key_map.add_key(&path);

        let name_hash = hash::murmur_hash(path.as_bytes());
        let filter_name = |name: u64| -> bool {
            match path.as_str() {
                "*" => true,
                _ => name == name_hash
            }
        };

        let mut num_files = 0;
        let mut set = HashSet::new();
        let mut bundles = Vec::<(u64, &mut BundleVersion, Vec<(u64, u64)>)>::new();
        for bundle in &mut self.bundles {
            let hash = bundle.hash();
            let mut group: Vec<(Patch, Vec<(u64, u64)>)> = Vec::new();
            for (patch, file) in bundle.active_files() {
                let key = (file.ext_hash(), file.name_hash());
                if !filter_extension(key.0) || !filter_name(key.1) {
                    continue;
                }

                if !set.contains(&key) {
                    set.insert(key);
                    if hash_fallback || can_file_self_name(file.ext_hash()) {
                        num_files += 1;
                    } else if let Some(_) = self.key_map.get_key(file.name_hash()) {
                        num_files += 1;
                    }

                    match group.binary_search_by(|(version_patch, ..)| version_patch.cmp(&patch)) {
                        Ok(i) => group.get_mut(i).unwrap().1.push(key),
                        Err(i) => group.insert(i, (patch, vec![key])),
                    }
                }
            }
            if !group.is_empty() {
                let mut versions = bundle.versions_mut();
                for (patch, files) in group {
                    let patch = Patch::from(patch);

                    if let Ok(i) = versions.binary_search_by(|probe| probe.patch().cmp(&patch)) {
                        let version = versions.remove(i);
                        bundles.push((hash, version, files));
                    } else {
                        eprintln!("bundle version does not exist");
                    }
                }
            }
        }

        if bundles.is_empty() {
            eprintln!("No matches for pattern \"{}\"", pattern);

            if let Some(ref send) = send {
                send.send(IndexEvent::End).unwrap();
            }

            return Ok(());
        }

        let dir = &self.dir;
        let filter = bundles.iter().map(|(hash, version, ..)| (*hash, version.patch())).collect::<HashSet<_>>();
        let files = scan_dir_filter(dir,
            |(bundle, patch, _)| filter.contains(&(*bundle, Patch::from(*patch))));
        if files.is_empty() {
            return Ok(());
        }

        let bundles = &Mutex::new(bundles);
        let count = &AtomicU32::new(0);
        let reader = &Reader::new(self.is_ssd);
        let key_map = &self.key_map;

        crossbeam_utils::thread::scope(|s| {
            reader.open_bundles(s, dir, files, num_threads, unbuffered);

            let mut threads = Vec::with_capacity(num_threads);
            for _ in 0..num_threads {
                let send = send.as_ref().cloned();
                threads.push(s.spawn(move |_| {
                    let mut read_buffer = ReadBuffer::new(ReadBuffer::CHUNK_SIZE * 4);
                    let mut hash_buffer = String::with_capacity(16);
                    let mut ext_buffer = String::with_capacity(16);
                    let mut path_buffer = PathBuf::with_capacity(512);

                    while let Some((mut fd, bundle_hash, patch, is_ssd)) = reader.pop() {
                        let (_, version, files) = {
                            let mut bundles = bundles.lock().unwrap();
                            if let Ok(i) = bundles.binary_search_by(|(version_hash, version, ..)| {
                                (*version_hash, version.patch()).cmp(&(bundle_hash, Patch::from(patch)))
                            }) {
                                bundles.remove(i)
                            } else {
                                panic!("missing data")
                            }
                        };

                        let reader = version.reader_mut();
                        reader.ssd_accelerator(is_ssd);

                        #[cfg(target_os = "windows")]
                        reader.unbuffered(unbuffered);

                        let mut files_read = 0;
                        let mut read = 0;
                        for (ext_hash, hash) in &files {
                            let name_key = key_map.get_key(*hash);
                            let ext_key = key_map.get_key(*ext_hash);

                            // ignore unknown extensions
                            if ext_key.is_some()
                                && (hash_fallback
                                    || name_key.is_some()
                                    || can_file_self_name(*ext_hash))
                            {
                                let buffer = version.read_file(
                                    &mut fd,
                                    bundle_hash,
                                    *ext_hash,
                                    *hash,
                                    &mut read_buffer,
                                ).unwrap_or_else(|e| panic!("bundle \"{}\" failed index with error: {}",
                                    format_bundle(*hash, patch),
                                    e));

                                read += buffer.len() as u64;

                                if let Some(out_dir) = out_dir {
                                    let mut i_file = get_file_interface(buffer).unwrap();

                                    let (self_name, self_ext) = i_file.path();

                                    let name = match name_key {
                                        Some(name) => name,
                                        None => {
                                            if let Some(name) = self_name {
                                                name
                                            } else if hash_fallback {
                                                hash_buffer.clear();
                                                write!(hash_buffer, "{:016x}", *hash).unwrap();
                                                hash_buffer.as_str()
                                            } else {
                                                continue
                                            }
                                        }
                                    };

                                    let ext = match self_ext {
                                        Some(ext) => ext,
                                        None => {
                                            if let Some(ext) = ext_key {
                                                ext
                                            } else if hash_fallback {
                                                ext_buffer.clear();
                                                write!(ext_buffer, "{:016x}", *hash).unwrap();
                                                ext_buffer.as_str()
                                            } else {
                                                continue
                                            }
                                        }
                                    };

                                    path_buffer.clear();
                                    path_buffer.push(out_dir);
                                    path_buffer.push(name);
                                    path_buffer.set_extension(ext);

                                    if let Some(dir) = path_buffer.parent() {
                                        //println!("{}", dir.display());
                                        std::fs::create_dir_all(&dir).unwrap();
                                    }

                                    let mut fd = OpenOptions::new()
                                        .write(true)
                                        .truncate(true)
                                        .create(true)
                                        .open(&path_buffer).unwrap();
                                    i_file.decompile(&mut fd).unwrap();
                                }
                                files_read += 1;
                            }
                        }

                        if let Some(ref send) = send {
                            send.send(IndexEvent::Progress {
                                read,
                                count: files_read + count.fetch_add(files_read, Ordering::Relaxed),
                            }).unwrap();
                        }
                    }
                }));
            }

            if let Some(ref send) = send {
                send.send(IndexEvent::Size(num_files as u32)).unwrap();
            }

            for thread in threads {
                thread.join().unwrap();
            }
        }).unwrap();

        Ok(())
    }

    fn find_and_check_bundles(&mut self) -> Vec<(u64, Patch)> {
        let incremental = !self.bundles.is_empty();
        let dir = &self.dir;
        let timestamps = &mut self.timestamps;

        if incremental {
            let mut new_timestamps = HashMap::with_capacity(timestamps.len());

            let bundles = &mut self.bundles;
            let files = scan_dir_filter(dir, |(hash, patch, metadata)| {
                let time = metadata
                    .modified().unwrap()
                    .duration_since(std::time::UNIX_EPOCH).unwrap()
                    .as_secs();

                if let Some(prev_time) = timestamps.remove(&(*hash, *patch)) {
                    new_timestamps.insert((*hash, *patch), time);
                    if time != prev_time {
                        match bundles.binary_search_by(|probe| probe.hash().cmp(hash)) {
                            Ok(i) => {
                                let bundle = bundles.get_mut(i).unwrap();
                                bundle.remove_version(*patch);
                            }
                            Err(_) => panic!("no existing bundle matching parallel timestamp"),
                        }
                        true
                    } else {
                        false
                    }
                } else {
                    new_timestamps.insert((*hash, *patch), time);
                    true
                }
            });

            std::mem::swap(&mut new_timestamps, timestamps);
            let old_timestamps = new_timestamps;
            for ((hash, patch), ..) in old_timestamps {
                match bundles.binary_search_by(|probe| probe.hash().cmp(&hash)) {
                    Ok(i) => {
                        let bundle = bundles.get_mut(i).unwrap();
                        bundle.remove_version(patch);
                    }
                    Err(_) => panic!("no bundle matching parallel timestamp for removal"),
                }
            }

            files
        } else {
            timestamps.clear();

            scan_dir_filter(dir, |(hash, patch, metadata)| {
                let time = metadata
                    .modified().unwrap()
                    .duration_since(std::time::UNIX_EPOCH).unwrap()
                    .as_secs();

                timestamps.insert((*hash, *patch), time);

                true
            })
        }
    }
}

pub fn decompress_bundle(fd: &mut File, target: &mut File) {
    let chunk_size = ReadBuffer::CHUNK_SIZE as u64;

    let (tx, rx) = mpsc::channel();

    let start = Instant::now();
    println!();
    println!("Dumping bundle...");

    let t = thread::spawn(move || -> io::Result<()> {
        let (read, _count, _size) = load_bar(rx)?;

        let millis = start.elapsed().as_millis();
        println!(
            "Extracted {} in {}.{:02} seconds",
            size_to_string(read),
            millis / 1000,
            (millis % 1000) / 10
        );

        Ok(())
    });

    let mut buffer = ReadBuffer::default();

    let mut reader = BundleReader::new();

    // workaround to get BundleReader to read bundle's uncompressed size
    reader.read(fd, &mut buffer, 0..256, None).unwrap();

    tx.send(IndexEvent::Size(reader.size() as u32)).unwrap();

    target.set_len(reader.size()).unwrap();
    let last = reader.size() % chunk_size;

    let mut chunks = reader.size() / chunk_size;
    if last > 0 { chunks += 1; }

    let mut total = 0;
    for i in 0..chunks {
        let offset = (i * chunk_size) as usize;

        let size = if i == chunks - 1 {
            last
        } else {
            chunk_size
        } as usize;

        let out = reader.read(fd, &mut buffer, offset..offset + size, None).unwrap();

        target.write_all(out).unwrap();

        total += size;
        tx.send(IndexEvent::Progress {
            read: size as u64,
            count: total as u32,
        }).unwrap();
    }

    tx.send(IndexEvent::End).unwrap();

    t.join().unwrap().unwrap();
}

fn load_bar(rx: mpsc::Receiver<IndexEvent>) -> io::Result<(u64, u64, u64)> {
    let start = Instant::now();

    let stdout = std::io::stdout();
    let mut old_str_len = 0;
    let mut progress_str = String::new();
    let mut total_size = None;
    let mut total_read = 0;
    let mut total_count = 0;

    let mut record = std::collections::VecDeque::with_capacity(50);
    let mut last = 0;
    let mut last_time = 0;
    loop {
        let mut is_done = false;
        let started = start.elapsed().as_millis();
        loop {
            let current = start.elapsed().as_millis();
            if total_read > 0 && started + 5 < current {
                break;
            }

            match rx.recv_timeout(Duration::from_millis(1)) {
                Ok(event) => match event {
                    IndexEvent::Size(size) => total_size = Some(size),
                    IndexEvent::Progress { read, count } => {
                        total_read += read;
                        total_count = count as u64;
                    }
                    IndexEvent::End => break is_done = true,
                },
                Err(mpsc::RecvTimeoutError::Disconnected) => break is_done = true,
                _ => (),
            }
        };

        let percent = if let Some(total_size) = total_size {
            total_count as f64 / total_size as f64
        } else { 0. };

        if record.is_empty() {
            last = total_read;
            last_time = start.elapsed().as_millis();
            record.push_back((last, last_time));
        } else {
            record.push_back((total_read - last, start.elapsed().as_millis() - last_time));
            last = total_read;
            last_time = start.elapsed().as_millis();
        };

        if record.len() > 200 {
            record.pop_front();
        }

        let mut bytes = 0;
        let mut time = 1;
        for (b, t) in &record {
            bytes += b;
            time += t;
        }
        let per_second = (bytes * 1000) / time as u64;

        progress_str.clear();
        write!(progress_str,
            "[{: <50}] {}% ({}/s)",
            "=".repeat((percent * 50.) as usize),
            (percent * 100.) as u8,
            size_to_string(per_second)
        ).unwrap();

        if old_str_len > progress_str.len() {
            for _ in progress_str.len()..old_str_len {
                progress_str.push(0x20 as char);
            }
        }

        {
            let mut out = stdout.lock();
            write!(out, "{}{}", progress_str, "\u{8}".repeat(progress_str.len()))?;
            out.flush()?;
        }

        old_str_len = progress_str.len();
        thread::sleep(Duration::from_millis(5));

        if is_done {
            writeln!(stdout.lock())?;
            break;
        }
    }

    let total_size = total_size.map(|n| n as u64);
    Ok((total_read, total_count, total_size.unwrap_or(total_count)))
}




