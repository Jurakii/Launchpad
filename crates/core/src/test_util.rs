use std::path::{Path, PathBuf};

/// A fresh folder standing in for the data folder; removed on drop.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn temp_dir() -> TempDir {
    let dir = std::env::temp_dir().join(format!("launchpad-test-{}", crate::ids::new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    TempDir(dir)
}

/// File names in `dir`, sorted (the JS tests' `listDir`).
#[allow(dead_code)]
pub fn list_dir(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}
