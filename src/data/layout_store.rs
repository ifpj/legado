//! Persisted exact-program layouts.
//!
//! An entry is the same normalised compile output as one element of
//! `profiles/protocol-layouts-v1.json`: wire format, key material and the
//! structural profiles of one initializer build. It never holds request
//! configuration, cookies, dynamic task inputs or answers, and a loaded entry
//! is matched by the current program digest, build and helper strings exactly
//! like an embedded one. Entries are bound to the binary that compiled them,
//! so a different build of this service recompiles instead of trusting files
//! produced by another compiler.
use super::normalize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::SystemTime,
};

pub(crate) const FORMAT_VERSION: u64 = 1;
/// Same bound as the in-memory cache; older files are pruned first.
pub(crate) const MAX_ENTRIES: usize = 32;
const MAX_FILE_BYTES: u64 = 1024 * 1024;

pub(crate) struct LayoutStore {
    dir: PathBuf,
    compiler: String,
    loaded: AtomicUsize,
    saved: AtomicUsize,
    rejected: AtomicUsize,
    last_error: Mutex<Option<String>>,
}

impl LayoutStore {
    /// Open or create the directory. On Unix the directory must be private to
    /// this user so another local account cannot pre-seed layouts.
    pub(crate) fn open(dir: &Path, compiler: &str) -> std::result::Result<Self, String> {
        if compiler.is_empty() || compiler == "unknown" {
            return Err("binary fingerprint unavailable".into());
        }
        match fs::metadata(dir) {
            Ok(meta) if meta.is_dir() => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if meta.permissions().mode() & 0o077 != 0 {
                        return Err(format!(
                            "{} must not be readable or writable by other users",
                            dir.display()
                        ));
                    }
                }
            }
            Ok(_) => return Err(format!("{} is not a directory", dir.display())),
            Err(_) => {
                create_private_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            }
        }
        Ok(Self {
            dir: dir.to_path_buf(),
            compiler: compiler.to_string(),
            loaded: AtomicUsize::new(0),
            saved: AtomicUsize::new(0),
            rejected: AtomicUsize::new(0),
            last_error: Mutex::new(None),
        })
    }
    fn note(&self, error: String) {
        self.rejected.fetch_add(1, Ordering::Relaxed);
        *self.last_error.lock().unwrap_or_else(|e| e.into_inner()) = Some(error);
    }
    fn entries(&self) -> Vec<(PathBuf, SystemTime, u64)> {
        let Ok(read) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut out: Vec<_> = read
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| {
                let meta = e.metadata().ok()?;
                if !meta.is_file() {
                    return None;
                }
                let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                Some((e.path(), modified, meta.len()))
            })
            .collect();
        out.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
        out
    }
    /// Read every valid entry written by this binary. Invalid files are
    /// counted and skipped; they never abort startup.
    pub(crate) fn load(&self) -> Vec<(bool, Value)> {
        let mut layouts: Vec<(bool, Value)> = Vec::new();
        for (path, _, size) in self.entries() {
            match self.read_entry(&path, size) {
                Ok(Some(entry)) => {
                    if !layouts.contains(&entry) {
                        layouts.push(entry);
                    }
                }
                Ok(None) => {}
                Err(error) => self.note(format!("{}: {error}", path.display())),
            }
        }
        self.loaded.store(layouts.len(), Ordering::Relaxed);
        layouts
    }
    fn read_entry(
        &self,
        path: &Path,
        size: u64,
    ) -> std::result::Result<Option<(bool, Value)>, String> {
        if size > MAX_FILE_BYTES {
            return Err("entry exceeds size limit".into());
        }
        let text = fs::read(path).map_err(|e| e.to_string())?;
        let value: Value = serde_json::from_slice(&text).map_err(|e| e.to_string())?;
        if value["cacheVersion"] != FORMAT_VERSION {
            return Err("unsupported cache version".into());
        }
        if value["compiler"] != self.compiler {
            // Another binary compiled this; it is neither trusted nor an error.
            return Ok(None);
        }
        let frame = value["frameTasks"]
            .as_bool()
            .ok_or_else(|| "frameTasks must be a boolean".to_string())?;
        let plan = normalize(&value["plan"]).map_err(|e| e.message)?;
        if !plan.is_object() || plan["format"].is_null() || plan["key_material"].is_null() {
            return Err("entry is not a compiled layout".into());
        }
        Ok(Some((frame, plan)))
    }
    /// Persist one newly compiled layout. Identical content maps to the same
    /// file name, so repeated compiles of one build never duplicate.
    pub(crate) fn save(&self, frame: bool, layout: &Value) {
        if let Err(error) = self.try_save(frame, layout) {
            self.note(error);
        }
    }
    fn try_save(&self, frame: bool, layout: &Value) -> std::result::Result<(), String> {
        let body = serde_json::to_vec(&json!({
            "cacheVersion": FORMAT_VERSION,
            "compiler": self.compiler,
            "frameTasks": frame,
            "plan": layout,
        }))
        .map_err(|e| e.to_string())?;
        if body.len() as u64 > MAX_FILE_BYTES {
            return Err("layout exceeds size limit".into());
        }
        let digest = hex::encode(Sha256::digest(&body));
        let kind = if frame { "frame" } else { "parent" };
        let path = self.dir.join(format!("{kind}-{}.json", &digest[..16]));
        if path.exists() {
            return Ok(());
        }
        let temp = self.dir.join(format!(
            ".{kind}-{}.{}.tmp",
            &digest[..16],
            std::process::id()
        ));
        fs::write(&temp, &body).map_err(|e| format!("{}: {e}", temp.display()))?;
        if let Err(error) = fs::rename(&temp, &path) {
            let _ = fs::remove_file(&temp);
            if !path.exists() {
                return Err(format!("{}: {error}", path.display()));
            }
        }
        self.saved.fetch_add(1, Ordering::Relaxed);
        self.prune();
        Ok(())
    }
    fn prune(&self) {
        let entries = self.entries();
        if entries.len() <= MAX_ENTRIES {
            return;
        }
        for (path, _, _) in &entries[..entries.len() - MAX_ENTRIES] {
            if let Err(error) = fs::remove_file(path) {
                self.note(format!("{}: {error}", path.display()));
            }
        }
    }
    pub(crate) fn status(&self) -> Value {
        json!({
            "enabled": true,
            "dir": self.dir.display().to_string(),
            "compiler": self.compiler,
            "loaded": self.loaded.load(Ordering::Relaxed),
            "saved": self.saved.load(Ordering::Relaxed),
            "rejected": self.rejected.load(Ordering::Relaxed),
            "lastError": self.last_error.lock().unwrap_or_else(|e| e.into_inner()).clone(),
        })
    }
}

fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(dir)
    }
}

static STORE: OnceLock<(Option<Arc<LayoutStore>>, Option<String>)> = OnceLock::new();

/// Choose the process-wide store once, before the first layout is read.
/// `None` disables persistence. Returns the status that is also reported by
/// `/v1/status`; a directory that cannot be used only disables the store.
pub fn configure(dir: Option<PathBuf>) -> Value {
    STORE.get_or_init(|| match dir {
        None => (None, Some("disabled".into())),
        Some(dir) => match LayoutStore::open(&dir, crate::version::build_id()) {
            Ok(store) => (Some(Arc::new(store)), None),
            Err(error) => (None, Some(error)),
        },
    });
    super::warm_layouts();
    status()
}
pub(crate) fn store() -> Option<Arc<LayoutStore>> {
    STORE.get().and_then(|(store, _)| store.clone())
}
pub fn status() -> Value {
    match STORE.get() {
        Some((Some(store), _)) => store.status(),
        Some((None, reason)) => json!({"enabled": false, "reason": reason}),
        None => json!({"enabled": false, "reason": "not configured"}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn temp_dir(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("cf-layout-store-{label}-{}", uuid::Uuid::new_v4()));
        dir
    }
    fn layout(shape: u64) -> Value {
        json!({"build":"abc123","format":{"byte_offset":1},"key_material":{"program_sha256":"deadbeef","helper":"h"},"shape":shape})
    }
    #[test]
    fn round_trip_dedups_identical_layouts_and_keeps_frame_flag() {
        let dir = temp_dir("round-trip");
        let store = LayoutStore::open(&dir, "build-a").unwrap();
        store.save(true, &layout(1));
        store.save(true, &layout(1));
        store.save(false, &layout(1));
        assert_eq!(store.entries().len(), 2);
        let again = LayoutStore::open(&dir, "build-a").unwrap();
        let mut loaded = again.load();
        loaded.sort_by_key(|(frame, _)| *frame);
        assert_eq!(loaded, vec![(false, layout(1)), (true, layout(1))]);
        assert_eq!(again.status()["loaded"], 2);
        assert_eq!(again.status()["rejected"], 0);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn foreign_compiler_corrupt_oversize_and_invalid_entries_are_skipped() {
        let dir = temp_dir("reject");
        let store = LayoutStore::open(&dir, "build-a").unwrap();
        store.save(false, &layout(1));
        let other = LayoutStore::open(&dir, "build-b").unwrap();
        other.save(false, &layout(2));
        fs::write(dir.join("corrupt.json"), b"{not json").unwrap();
        fs::write(
            dir.join("version.json"),
            serde_json::to_vec(&json!({"cacheVersion":99,"compiler":"build-a","frameTasks":false,"plan":layout(3)})).unwrap(),
        )
        .unwrap();
        fs::write(
            dir.join("shape.json"),
            serde_json::to_vec(&json!({"cacheVersion":1,"compiler":"build-a","frameTasks":false,"plan":{"declared":{"declarationKind":"bogus"},"format":{},"key_material":{}}})).unwrap(),
        )
        .unwrap();
        fs::write(
            dir.join("empty.json"),
            serde_json::to_vec(
                &json!({"cacheVersion":1,"compiler":"build-a","frameTasks":"no","plan":layout(4)}),
            )
            .unwrap(),
        )
        .unwrap();
        fs::write(
            dir.join("large.json"),
            vec![b' '; MAX_FILE_BYTES as usize + 1],
        )
        .unwrap();
        let reader = LayoutStore::open(&dir, "build-a").unwrap();
        assert_eq!(reader.load(), vec![(false, layout(1))]);
        let status = reader.status();
        assert_eq!(status["loaded"], 1);
        assert_eq!(status["rejected"], 5);
        assert!(status["lastError"].is_string());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn oldest_entries_are_pruned_beyond_the_bound() {
        let dir = temp_dir("prune");
        let store = LayoutStore::open(&dir, "build-a").unwrap();
        for shape in 0..(MAX_ENTRIES as u64 + 3) {
            let before: Vec<_> = store.entries().into_iter().map(|e| e.0).collect();
            store.save(false, &layout(shape));
            // Distinct modification times keep the pruning order observable.
            let written = store
                .entries()
                .into_iter()
                .map(|e| e.0)
                .find(|p| !before.contains(p))
                .unwrap();
            let later =
                SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000 + shape);
            fs::File::options()
                .write(true)
                .open(&written)
                .unwrap()
                .set_modified(later)
                .unwrap();
        }
        assert_eq!(store.entries().len(), MAX_ENTRIES);
        let loaded = LayoutStore::open(&dir, "build-a").unwrap().load();
        let shapes: Vec<u64> = loaded
            .iter()
            .map(|(_, v)| v["shape"].as_u64().unwrap())
            .collect();
        assert_eq!(shapes.len(), MAX_ENTRIES);
        assert!(!shapes.contains(&0) && !shapes.contains(&1) && !shapes.contains(&2));
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn unusable_directory_only_disables_the_store() {
        let file = temp_dir("file");
        fs::write(&file, b"x").unwrap();
        assert!(LayoutStore::open(&file, "build-a").is_err());
        assert!(LayoutStore::open(&file.join("child"), "build-a").is_err());
        assert!(LayoutStore::open(&temp_dir("unknown"), "unknown").is_err());
        fs::remove_file(file).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn shared_directories_are_rejected_on_unix() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("shared");
        fs::create_dir_all(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(LayoutStore::open(&dir, "build-a").is_err());
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(LayoutStore::open(&dir, "build-a").is_ok());
        let created = temp_dir("created");
        LayoutStore::open(&created, "build-a").unwrap();
        assert_eq!(
            fs::metadata(&created).unwrap().permissions().mode() & 0o777,
            0o700
        );
        fs::remove_dir_all(dir).unwrap();
        fs::remove_dir_all(created).unwrap();
    }
}
