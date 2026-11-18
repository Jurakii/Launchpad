//! Carrying out `core::items::IconTask`s: each writes icons\<id>.png, then
//! `items::apply_icon` records it. `spawn` does a batch on a worker thread;
//! `run` is the blocking piece (a custom icon must be saved before the list is).
use super::App;
use crate::win::image::{large_icon_size, Format, Wic};
use crate::win::services::Icons;
use launchpad_core::items::{self, IconTask};
use launchpad_core::{favicon, Error, Result};
use std::path::{Path, PathBuf};

/// Does one task; the written file's path, or None when there was nothing
/// to get (no icon on the site, a file without one).
pub fn run(task: &IconTask, icons_dir: &Path, google_fallback: bool) -> Result<Option<String>> {
    let png = match task {
        IconTask::SaveCustom { source, .. } => {
            Some(Wic::new()?.custom_icon_png(Path::new(source)).map_err(|_| Error::msg("Could not read image file"))?)
        }
        IconTask::Extract { path, .. } => {
            let wic = Wic::new()?;
            match wic.file_icon(Path::new(path), large_icon_size()) {
                Ok(img) => Some(wic.encode_rgba(&img, Format::Png)?),
                Err(_) => None,
            }
        }
        IconTask::Favicon { url, .. } => {
            let mut src = Icons::new()?;
            match favicon::fetch_favicon(url, google_fallback, &mut src) {
                Some(img) => Some(src.png(&img)?),
                None => None,
            }
        }
    };
    let Some(png) = png else { return Ok(None) };
    std::fs::create_dir_all(icons_dir).map_err(|e| Error::msg(format!("Creating {}: {e}", icons_dir.display())))?;
    let dest: PathBuf = icons_dir.join(format!("{}.png", task.id()));
    std::fs::write(&dest, png).map_err(|e| Error::msg(format!("Writing {}: {e}", dest.display())))?;
    Ok(Some(dest.to_string_lossy().into_owned()))
}

/// Whether "Use Google to find website icons as a fallback" is on (1.2's default: on).
pub fn google_fallback(app: &App) -> bool {
    app.settings().get("googleFaviconFallback") != Some(&serde_json::Value::Bool(false))
}

/// Runs `tasks` one after another on a worker thread and records each icon as
/// it arrives. `done(fixed)` runs on the UI thread at the end, with how many
/// entries got a new icon.
pub fn spawn(app: &App, tasks: Vec<IconTask>, done: impl FnOnce(usize) + Send + 'static) {
    let dir = app.store.borrow().icons_dir();
    let google = google_fallback(app);
    std::thread::spawn(move || {
        let mut fixed = 0usize;
        for task in tasks {
            let path = run(&task, &dir, google).unwrap_or_else(|e| {
                eprintln!("Icon for {}: {e}", task.id());
                None
            });
            let Some(path) = path else { continue };
            let (tx, rx) = std::sync::mpsc::channel();
            super::post(move |app| {
                app.images.forget(Path::new(&path));
                let changed = app.mutate(|apps| Ok(items::apply_icon(apps, &task, Some(path)))).unwrap_or(false);
                let _ = tx.send(changed);
            });
            fixed += rx.recv().unwrap_or(false) as usize;
        }
        super::post(move |_| done(fixed));
    });
}
