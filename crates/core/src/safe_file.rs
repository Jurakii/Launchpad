//! Crash-safe writes for the data files. A plain write truncates first, so a
//! crash or power cut mid-save left a half-written `apps.json` that no longer
//! parsed.
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::Duration;

// Defender and the search indexer briefly hold a freshly written file open,
// which makes the rename-over fail for a few ms.
const RENAME_RETRIES: u32 = 5;
const RENAME_RETRY_DELAY: Duration = Duration::from_millis(25);

/// Writes `<file>.tmp`, flushes it to disk, then renames it over `file`. The
/// rename replaces the file in one step on NTFS, so readers see the old
/// contents or the new ones, never part of a write.
pub fn write_file_atomic(file: &Path, data: impl AsRef<[u8]>) -> io::Result<()> {
    let mut tmp = file.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    {
        let mut f = File::create(&tmp)?;
        f.write_all(data.as_ref())?;
        f.sync_all()?;
    }
    let mut attempt = 0;
    loop {
        match fs::rename(&tmp, file) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied && attempt < RENAME_RETRIES => {
                attempt += 1;
                sleep(RENAME_RETRY_DELAY);
            }
            Err(e) => {
                let _ = fs::remove_file(&tmp);
                return Err(e);
            }
        }
    }
}

/// Where to keep an unreadable data file so it isn't lost:
/// `apps.json` -> `apps.corrupt-2026-10-13T09-30-00-000Z.json`, same folder.
pub fn corrupt_copy_path(file: &Path) -> PathBuf {
    let stamp = crate::time::now_iso().replace([':', '.'], "-");
    let stem = file.file_stem().unwrap_or_default().to_string_lossy();
    let ext = file.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    file.with_file_name(format!("{stem}.corrupt-{stamp}{ext}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::temp_dir;

    #[test]
    fn replaces_the_file_and_leaves_no_tmp() {
        let dir = temp_dir();
        let file = dir.path().join("apps.json");
        write_file_atomic(&file, "one").unwrap();
        write_file_atomic(&file, "two").unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "two");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn corrupt_copy_is_named_beside_the_file() {
        let p = corrupt_copy_path(Path::new("C:/data/apps.json"));
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("apps.corrupt-20") && name.ends_with("Z.json"), "{name}");
        assert!(!name.contains(':'));
        assert_eq!(p.parent(), Some(Path::new("C:/data")));
    }
}
