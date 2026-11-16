use crate::sync::LockOk;
use crate::torrent::safe_part;
use anyhow::{bail, Context};
use librqbit::storage::filesystem::FilesystemStorageFactory;
use librqbit::storage::{StorageFactory, StorageFactoryExt, TorrentStorage};
use librqbit::{ManagedTorrentShared, TorrentMetadata};
use std::any::TypeId;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Where each file of a torrent goes, under the folder it is saved in: a
/// torrent of many files in a folder of its own name, every part of every
/// name made safe ("CON", "a:b", "x." can't be kept by Windows as they are).
pub fn disk_paths(metadata: &TorrentMetadata) -> Vec<Option<PathBuf>> {
    let info = metadata.info.info();
    let many = info.files.is_some();
    let top = metadata.info.name().map(|n| safe_part(&n)).unwrap_or_else(|| "torrent".into());
    metadata
        .info
        .iter_file_details()
        .zip(metadata.file_infos.iter())
        .map(|(fd, f)| {
            if f.attrs.padding {
                return None;
            }
            let mut p = PathBuf::new();
            if many {
                p.push(&top);
            }
            for part in fd.filename.iter_components() {
                p.push(safe_part(&part));
            }
            Some(p)
        })
        .collect()
}

/// Each torrent's folder (by its id): librqbit keeps it to itself.
pub(super) type Folders = Arc<Mutex<HashMap<String, PathBuf>>>;

/// Our file store for librqbit: files at their safe names.
#[derive(Clone, Default)]
pub struct SafeFiles {
    pub(super) folders: Folders,
}

impl StorageFactory for SafeFiles {
    type Storage = SafeStore;

    fn create(&self, shared: &ManagedTorrentShared, _metadata: &TorrentMetadata) -> anyhow::Result<SafeStore> {
        let root = self.folders.lock_ok().get(&shared.info_hash.as_string()).cloned().context("the torrent's folder is not known")?;
        Ok(SafeStore { root, files: Arc::new(Vec::new()) })
    }

    // librqbit keeps its notes of what is on the disk (to go on quickly) only
    // for plain files on the disk; these are such files, at safer names
    fn is_type_id(&self, type_id: TypeId) -> bool {
        type_id == TypeId::of::<Self>() || type_id == TypeId::of::<FilesystemStorageFactory>()
    }

    fn clone_box(&self) -> librqbit::storage::BoxStorageFactory {
        self.clone().boxed()
    }
}

/// One file of a torrent: where it goes, opened when first needed (the
/// files that were not chosen are mostly never made).
struct Slot {
    path: PathBuf,
    file: std::sync::OnceLock<File>,
}

/// The files of one torrent (none for the padding between files).
pub struct SafeStore {
    root: PathBuf,
    files: Arc<Vec<Option<Slot>>>,
}

impl SafeStore {
    fn slot(&self, id: usize) -> anyhow::Result<&Slot> {
        match self.files.get(id) {
            Some(Some(s)) => Ok(s),
            Some(None) => bail!("padding has no file"),
            None => bail!("no such file in the torrent"),
        }
    }

    /// The open file; `make`: made when it is not there yet.
    fn open(&self, id: usize, make: bool) -> anyhow::Result<&File> {
        let s = self.slot(id)?;
        if let Some(f) = s.file.get() {
            return Ok(f);
        }
        if !make && !s.path.exists() {
            bail!("{} is not there yet", s.path.display());
        }
        if let Some(dir) = s.path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("the folder {} can't be made", dir.display()))?;
        }
        let f = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&s.path)
            .with_context(|| format!("{} can't be opened", s.path.display()))?;
        // a piece written far into the file: no waiting for Windows to fill
        // everything before it with zeros
        crate::fileio::make_sparse(&f);
        let _ = s.file.set(f);
        s.file.get().context("the file was not kept")
    }
}

impl TorrentStorage for SafeStore {
    fn init(&mut self, _shared: &ManagedTorrentShared, metadata: &TorrentMetadata) -> anyhow::Result<()> {
        let root = self.root.clone();
        self.files = Arc::new(
            disk_paths(metadata)
                .into_iter()
                .map(|p| p.map(|p| Slot { path: root.join(p), file: Default::default() }))
                .collect(),
        );
        Ok(())
    }

    fn pread_exact(&self, file_id: usize, offset: u64, buf: &mut [u8]) -> anyhow::Result<()> {
        Ok(crate::fileio::read_at(self.open(file_id, false)?, buf, offset)?)
    }

    fn pwrite_all(&self, file_id: usize, offset: u64, buf: &[u8]) -> anyhow::Result<()> {
        Ok(crate::fileio::write_at(self.open(file_id, true)?, buf, offset)?)
    }

    fn remove_file(&self, file_id: usize, _filename: &Path) -> anyhow::Result<()> {
        match self.files.get(file_id) {
            Some(Some(s)) if s.path.exists() => Ok(std::fs::remove_file(&s.path)?),
            _ => Ok(()),
        }
    }

    fn remove_directory_if_empty(&self, path: &Path) -> anyhow::Result<()> {
        let dir = self.root.join(path);
        if dir.is_dir() && std::fs::read_dir(&dir)?.next().is_none() {
            std::fs::remove_dir(&dir)?;
        }
        Ok(())
    }

    fn ensure_file_length(&self, file_id: usize, length: u64) -> anyhow::Result<()> {
        if matches!(self.files.get(file_id), Some(None)) {
            return Ok(());
        }
        let f = self.open(file_id, true)?;
        if f.metadata()?.len() != length {
            f.set_len(length)?;
        }
        Ok(())
    }

    fn take(&self) -> anyhow::Result<Box<dyn TorrentStorage>> {
        Ok(Box::new(SafeStore { root: self.root.clone(), files: self.files.clone() }))
    }
}
