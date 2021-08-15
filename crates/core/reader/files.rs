use std::sync::{Mutex, Condvar};
use std::sync::atomic::{AtomicBool, Ordering};
use std::fs::{File, OpenOptions, Metadata};
use std::fs::read_dir;
use std::path::Path;

use crossbeam_utils::thread::Scope;
use stingray::get_bundle_hash_patch;

use crate::utility::format_bundle;

pub fn scan_dir_filter<P>(dir: &Path, mut filter: P) -> Vec<(u64, u16)>
    where
        P: FnMut(&(u64, u16, Metadata)) -> bool,// + std::marker::Send,
{
    let mut bundles = Vec::<(u64, u16)>::new();
    if let Ok(dir) = read_dir(dir) {
        for entry in dir.flatten() {
            if let Some(s) = entry.path().to_str() {
                if let Some((hash, patch)) = get_bundle_hash_patch(s) {
                    let patch = patch.get().unwrap_or(0);
                    let metadata = entry
                        .metadata().unwrap();

                    if filter(&(hash, patch, metadata)) {
                        bundles.push((hash, patch));
                    }
                }
            }
        }
    }
    bundles
}

pub struct Reader {
    files: Mutex<Vec<(LazyFile, Option<u64>, u64, u16)>>,
    num_files: Mutex<u64>,
    has_num: Condvar,

    ready: Condvar,
    sorted: AtomicBool,
    read: AtomicBool,
    done: AtomicBool,

    is_ssd: AtomicBool,
}

impl Reader {
    pub fn new(is_ssd: bool) -> Self {
        Self {
            files: Mutex::new(Vec::new()),
            num_files: Mutex::new(u64::MAX),
            has_num: Condvar::new(),
            ready: Condvar::new(),
            sorted: AtomicBool::new(false),
            read: AtomicBool::new(false),
            done: AtomicBool::new(false),
            is_ssd: AtomicBool::new(is_ssd),
        }
    }

    pub fn is_ssd(&self) -> bool {
        self.is_ssd.load(Ordering::SeqCst)
    }

    pub fn num_files(&self) -> u64 {
        *self.has_num.wait_while(self.num_files.lock().unwrap(), |num_files| *num_files == u64::MAX).unwrap()
    }

    pub fn pop(&self) -> Option<(File, u64, u16, bool)> {
        let is_ssd = self.is_ssd.load(Ordering::SeqCst);
        loop {
            if self.done.load(Ordering::SeqCst) {
                let mut files = self.files.lock().unwrap();
                break files.pop();
            } else {
                //let mut files = self.ready.wait(self.files.lock().unwrap()).unwrap();
                match self.ready.wait_timeout(self.files.lock().unwrap(), std::time::Duration::from_millis(2)) {
                    Err(_) => continue,
                    Ok((mut files, _)) => {
                        if files.len() == 0 {
                            //eprintln!("files.len() is 0 but Reader isn't done reading files");
                            continue;
                        }

                        break files.pop();
                    }
                }
            }
        }.map(|(lazy, _, hash, patch)| (lazy.open(), hash, patch, is_ssd))
    }

    pub fn open_bundles<'a>(
        &'a self,
        scope: &Scope<'a>,
        dir: &'a Path,
        mut bundles: Vec<(u64, u16)>,
        num_threads: usize,
        unbuffered: bool,
    ) {
        // async adds overhead on big reads for is_ssd files
        // since files can be captured before is_ssd is set
        // and reads will buffer on reads from an SSD
        scope.spawn(move |_| {
            if let Some(is_ssd) = drive::in_ssd(dir) {
                if !is_ssd && cfg!(windows) {
                    let mut files = {
                        let mut files = self.files.lock().unwrap();
                        std::mem::take(&mut (*files))
                    };

                    for (fd, offset, ..) in &mut files {
                        *offset = drive::file_offset(&fd.borrow());
                        assert!(offset.is_some());
                    }

                    files.sort_by(|(_, a, ..), (_, b, ..)| {
                        sort_offset(a, b)
                    });

                    {
                        let mut files2 = self.files.lock().unwrap();
                        files2.reserve(files.len());
                        for file in files {
                            match files2.binary_search_by(|(_, probe, ..)| sort_offset(probe, &file.1)) {
                                Err(i) => files2.insert(i, file),
                                Ok(i) => files2.insert(i, file),
                            }
                        }
                    }
                }
                self.is_ssd.store(is_ssd, Ordering::SeqCst);
            }
            self.sorted.store(true, Ordering::SeqCst);
            if self.read.load(Ordering::SeqCst) {
                self.done.store(true, Ordering::SeqCst);
            }
        });

        scope.spawn(move |_| {
            if !bundles.is_empty() {
                {
                    *self.num_files.lock().unwrap() = bundles.len() as u64;
                    self.has_num.notify_all();
                }

                let num_threads = if bundles.len() > num_threads {
                    num_threads
                } else {
                    bundles.len()
                };
                let thread_size = bundles.len() / num_threads;

                crossbeam_utils::thread::scope(|s| {
                    let mut split = &mut bundles[..];
                    for i in 0..num_threads {
                        let end = match i + 1 {
                            x if x == num_threads => split.len(),
                            _ => thread_size,
                        };

                        let (bundles, r) = split.split_at_mut(end);
                        split = r;

                        let mut path = dir.to_owned();
                        s.spawn(move |_| {
                            for (bundle_hash, patch) in bundles {
                                path.push(format_bundle(*bundle_hash, *patch));

                                let lazy = LazyFile::new(&path, unbuffered);

                                {
                                    #[cfg(target_os = "windows")]
                                    if self.is_ssd.load(Ordering::SeqCst) {
                                        let mut files = self.files.lock().unwrap();
                                        files.push((lazy, None, *bundle_hash, *patch));
                                    } else {
                                        let offset = drive::file_offset(&lazy.borrow());
                                        let mut files = self.files.lock().unwrap();
                                        if offset.is_none() {
                                            files.push((lazy, None, *bundle_hash, *patch));
                                        } else {
                                            match files.binary_search_by(|(_, probe, ..)| sort_offset(probe, &offset)) {
                                                Err(i) => files.insert(i, (lazy, offset, *bundle_hash, *patch)),
                                                Ok(i) => files.insert(i, (lazy, offset, *bundle_hash, *patch)),
                                            }
                                        }
                                    }

                                    #[cfg(not(target_os = "windows"))]
                                    {
                                        let mut files = self.files.lock().unwrap();
                                        files.push((lazy, None, *bundle_hash, *patch));
                                    }
                                    self.ready.notify_one();
                                }
                                path.pop();
                            }
                        });
                    }
                }).unwrap();
            }

            self.read.store(true, Ordering::SeqCst);
            if self.sorted.load(Ordering::SeqCst) {
                //println!("READ-DONE");
                self.done.store(true, Ordering::SeqCst);
            }
            self.ready.notify_all();
        });
    }
}

fn sort_offset(a: &Option<u64>, b: &Option<u64>) -> std::cmp::Ordering {
    (*b).cmp(a)
}



// LazyFiles is a workaround to caching open file handles on Linux
// WSL2 had 1024 and 4096 file descriptor soft and hard limits respectively
// as of 7/30/2021 Vermintide 2 has 11495 bundles
//
// TODO try other caching strategies on Linux
#[cfg(target_os = "windows")]
use std::os::windows::prelude::*;

#[cfg(target_os = "windows")]
struct LazyFile {
    fd: File,
}

#[cfg(target_os = "windows")]
impl LazyFile {
    fn new(path: &Path, unbuffered: bool) -> Self {
        let fd = if unbuffered {
            OpenOptions::new()
                .read(true)
                .attributes(0x20000000) //FILE_FLAG_NO_BUFFERING
                .open(path).unwrap()
        } else {
            OpenOptions::new()
                .read(true)
                .open(path).unwrap()
        };

        Self {
            fd,
        }
    }

    fn borrow(&self) -> &File {
        &self.fd
    }

    fn open(self) -> File {
        self.fd
    }
}

#[cfg(not(target_os = "windows"))]
use std::path::PathBuf;

#[cfg(not(target_os = "windows"))]
struct LazyFile {
    path: PathBuf,
}

#[cfg(not(target_os = "windows"))]
impl LazyFile {
    fn new(path: &Path, _unbuffered: bool) -> Self {
        Self {
            path: path.to_owned(),
        }
    }

    fn borrow(&self) -> File {
        self.open()
    }

    fn open(&self) -> File {
        OpenOptions::new()
            .read(true)
            .open(&self.path).unwrap()
    }
}

