use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);
const CREATE_ATTEMPTS: usize = 1_024;

pub struct OwnedTempDir {
    root: PathBuf,
}

impl OwnedTempDir {
    pub fn new(prefix: &str) -> Self {
        for _ in 0..CREATE_ATTEMPTS {
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let root =
                std::env::temp_dir().join(format!("{}_{}_{}", prefix, std::process::id(), id));
            match fs::create_dir(&root) {
                Ok(()) => return Self { root },
                Err(err) if err.kind() == ErrorKind::AlreadyExists => continue,
                Err(err) => panic!("create temporary directory {}: {err}", root.display()),
            }
        }

        panic!("could not allocate a unique temporary directory for {prefix}");
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
}

impl AsRef<Path> for OwnedTempDir {
    fn as_ref(&self) -> &Path {
        &self.root
    }
}

impl Drop for OwnedTempDir {
    fn drop(&mut self) {
        if let Err(err) = fs::remove_dir_all(&self.root) {
            if err.kind() != ErrorKind::NotFound {
                eprintln!(
                    "failed to remove temporary directory {}: {err}",
                    self.root.display()
                );
            }
        }
    }
}
