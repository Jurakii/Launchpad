//! The edits the main window makes to apps.json, as plain functions over the
//! loaded list. The caller loads with `store::Store::load`, calls one of
//! these, saves, and then does whatever I/O the return value asks for
//! (icons, stored copies). Ported from main.js's IPC handlers:
//!
//! | here | main.js |
//! |---|---|
//! | `validate_form` | the add/edit dialog's checks (renderer.js `appForm` submit) |
//! | `is_shortcut`, `apply_shortcut` | `resolveShortcutTarget`, `applyShortcutResolution` |
//! | `infer_type_from_ext` | `inferTypeFromExt` (`dialog:pickTarget`) |
//! | `descendant_island_ids` | `descendantIslandIds` |
//! | `icon_task`, `apply_icon` | `autoFetchIcon`, `icons.saveCustomIcon` callers |
//! | `add_app`, `update_app`, `remove` | `apps:add`, `apps:update`, `apps:remove` |
//! | `add_island` | `folder:add` |
//! | `rename`, `set_color`, `move_item`, `set_position`, `set_positions` | `item:rename`, `item:setColor`, `item:move`, `item:setPosition`, `item:setPositions` |
//! | `set_collapsed`, `set_show_names`, `set_size` | `item:setCollapsed`, `item:setShowNames`, `item:setSize` |
//! | `toggle_pin`, `set_pinned`, `set_pin_order` | `item:togglePin`, `item:setPinned`, `item:setPinOrder` |
//! | `launchable`, `record_launch` | `apps:launch`, `launchAndRecord` |
//! | `detect_missing_icons` | `apps:detectMissingIcons` |
//! | `first_run`, `restore_defaults`, `repair_defaults` | `buildDefaultAppEntries`/`fetchDefaultIcons`, `apps:restoreDefaults`, `apps:repairDefaults` |
//!
//! An id that matches nothing is a no-op, as in 1.2.2 (which saved and
//! returned the list unchanged).
use std::collections::HashSet;

use serde_json::{json, Value};

use crate::js::{js_num, js_round, js_trim};
use crate::layout;
use crate::store::{self, parent_key, strict_eq, Entry};
use crate::{defaults, Error, Result};

/// The add/edit dialog's fields, already trimmed as the dialog trims them.
/// `""` stands for JS's empty / undefined.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AppForm {
    pub name: String,
    pub type_: String,
    pub target: String,
    pub args: String,
    pub cwd: String,
    pub interpreter: String,
    pub command: String,
    /// The island the dialog was opened in; `""` is Home. Only used by `add_app`.
    pub parent_id: String,
    /// An image the user picked; `""` when none.
    pub custom_icon_path: String,
    /// "Auto-detect Icon" was clicked.
    pub force_auto_icon: bool,
}

/// What `read_shortcut` (the Windows layer) found in a .lnk.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shortcut {
    pub target: String,
    pub args: String,
    pub cwd: String,
}

/// Icon work the caller does after the edit, then reports with `apply_icon`.
#[derive(Clone, Debug, PartialEq)]
pub enum IconTask {
    /// Save the picked image `source` as icons\<id>.png (128x128). If that
    /// fails, don't save the list: 1.2.2 threw "Could not read image file"
    /// and kept nothing of the add or edit.
    SaveCustom { id: String, source: String },
    /// Fetch the site's favicon into icons\<id>.png (googleFallback from settings).
    Favicon { id: String, url: String },
    /// Extract the file's icon into icons\<id>.png.
    Extract { id: String, path: String },
}

impl IconTask {
    pub fn id(&self) -> &str {
        match self {
            IconTask::SaveCustom { id, .. } | IconTask::Favicon { id, .. } | IconTask::Extract { id, .. } => id,
        }
    }
}

/// After `remove`: delete icons\<id>.png if it exists, and when `stored`,
/// `file_store::remove_stored` (logging a failure). Both only after the save
/// succeeded, so a failed save never leaves an entry pointing at nothing.
#[derive(Clone, Debug, PartialEq)]
pub struct Removed {
    pub stored: bool,
}

/// What `restore_defaults` / `repair_defaults` did, for the UI's message.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DefaultsChange {
    pub repaired: usize,
    pub added: usize,
    pub icons: Vec<IconTask>,
}

fn find(apps: &[Entry], id: &str) -> Option<usize> {
    apps.iter().position(|a| a.str("id") == Some(id))
}

fn null_if_empty(s: &str) -> Value {
    if s.is_empty() {
        Value::Null
    } else {
        Value::from(s)
    }
}

/// The add/edit dialog refuses to submit with these, word for word.
pub fn validate_form(form: &AppForm) -> Result<()> {
    if form.name.is_empty() {
        return Err(Error::msg("Give it a name."));
    }
    if form.type_ != "command" && form.target.is_empty() {
        return Err(Error::msg("Set a target."));
    }
    if form.type_ == "command" && form.command.is_empty() {
        return Err(Error::msg("Enter a command."));
    }
    Ok(())
}

/// `path.win32.extname(p).toLowerCase()`.
fn ext_lower(p: &str) -> String {
    let base = p.rsplit(['/', '\\']).next().unwrap_or("");
    match base.rfind('.') {
        Some(i) if i > 0 => base[i..].to_lowercase(),
        _ => String::new(),
    }
}

/// Whether `apply_shortcut` would read this target as a shortcut.
pub fn is_shortcut(path: &str) -> bool {
    ext_lower(path) == ".lnk"
}

// `path.win32.resolve` as (root, segments), for comparing two absolute paths.
// shortcut: a relative path isn't joined to the process's cwd as Node does;
// IShellLink hands back absolute paths, so it never comes up.
fn resolve_parts(p: &str) -> (String, Vec<&str>) {
    let mut parts = p.split(['/', '\\']).peekable();
    let mut root = String::new();
    if p.starts_with("\\\\") || p.starts_with("//") {
        parts.next();
        parts.next();
        let server = parts.next().unwrap_or("");
        let share = parts.next().unwrap_or("");
        root = format!("\\\\{server}\\{share}\\");
    } else if p.len() >= 2 && p.as_bytes()[1] == b':' && p.is_char_boundary(2) {
        let drive = parts.next().unwrap_or("");
        root = format!("{drive}\\");
    }
    let mut segs = Vec::new();
    for s in parts {
        match s {
            "" | "." => {}
            ".." => {
                segs.pop();
            }
            s => segs.push(s),
        }
    }
    (root, segs)
}

/// A `path` item whose target is a .lnk gets what the shortcut points at
/// instead: its target, its arguments when it has any, and its start-in
/// folder when that isn't just the target's own folder. `resolve` is
/// `win::launch::read_shortcut`, `None` when the file can't be read; it's
/// only called for a .lnk.
pub fn apply_shortcut(entry: &mut Entry, resolve: impl FnOnce(&str) -> Option<Shortcut>) {
    if entry.str("type") != Some("path") {
        return;
    }
    let Some(target) = entry.str("target").filter(|t| is_shortcut(t)) else { return };
    let Some(info) = resolve(target).filter(|i| !i.target.is_empty()) else { return };
    let (root, mut dir) = resolve_parts(&info.target);
    dir.pop();
    let args = js_trim(&info.args);
    if !args.is_empty() {
        entry.set("args", args);
    }
    if !info.cwd.is_empty() && resolve_parts(&info.cwd) != (root, dir) {
        entry.set("cwd", info.cwd.as_str());
    }
    entry.set("target", info.target);
}

/// `(type, interpreter)` for a file picked as a target.
pub fn infer_type_from_ext(target: &str) -> (&'static str, &'static str) {
    match ext_lower(target).as_str() {
        ".py" => ("script", "python"),
        ".js" | ".mjs" => ("script", "node"),
        _ => ("path", ""),
    }
}

/// `island_id` and every island nested in it, however deep.
pub fn descendant_island_ids(apps: &[Entry], island_id: &str) -> HashSet<String> {
    let mut ids = HashSet::from([island_id.to_owned()]);
    let mut grew = true;
    while grew {
        grew = false;
        for a in apps {
            let (Some(id), Some(parent)) = (a.str("id"), a.str("parentId")) else { continue };
            if a.str("kind") == Some("island") && ids.contains(parent) && !ids.contains(id) {
                ids.insert(id.to_owned());
                grew = true;
            }
        }
    }
    ids
}

/// `autoFetchIcon`: a URL's favicon, anything else its target's icon.
pub fn icon_task(id: &str, entry: &Entry) -> IconTask {
    let target = entry.str("target").unwrap_or("").to_owned();
    match entry.str("type") {
        Some("url") => IconTask::Favicon { id: id.to_owned(), url: target },
        _ => IconTask::Extract { id: id.to_owned(), path: target },
    }
}

/// Records a finished `IconTask`: `path` is the icon file it wrote, `None`
/// when it found nothing (the old icon stays). A fetched icon clears
/// iconIsCustom. False when nothing changed (no icon, or the entry is gone).
pub fn apply_icon(apps: &mut [Entry], task: &IconTask, path: Option<String>) -> bool {
    let (Some(path), Some(i)) = (path, find(apps, task.id())) else { return false };
    apps[i].set("iconPath", path);
    apps[i].set("iconIsCustom", matches!(task, IconTask::SaveCustom { .. }));
    true
}

/// `apps:add`: a new app on the first free cell of its island. Returns the
/// icon to save or fetch, if any.
pub fn add_app(
    apps: &mut Vec<Entry>,
    form: &AppForm,
    id: String,
    now: i64,
    resolve: impl FnOnce(&str) -> Option<Shortcut>,
) -> Option<IconTask> {
    let or = |s: &str, fallback: &'static str| if s.is_empty() { fallback.to_owned() } else { s.to_owned() };
    let mut entry = Entry::from_value(json!({
        "id": id,
        "kind": "app",
        "name": or(&form.name, "Untitled"),
        "type": form.type_,
        "target": form.target,
        "args": form.args,
        "cwd": form.cwd,
        "interpreter": form.interpreter,
        "command": form.command,
        "parentId": null_if_empty(&form.parent_id),
        "pinned": false,
        "createdAt": now,
        "lastLaunched": null,
        "launchCount": 0,
        "color": null,
        "iconPath": null,
        "iconIsCustom": false,
    }))
    .expect("an object");
    apply_shortcut(&mut entry, resolve);
    layout::place_free(apps, &mut entry, None);

    let task = if !form.custom_icon_path.is_empty() {
        entry.set("iconIsCustom", true);
        Some(IconTask::SaveCustom { id: id.clone(), source: form.custom_icon_path.clone() })
    } else if entry.truthy("target") {
        Some(icon_task(&id, &entry))
    } else {
        None
    };
    apps.push(entry);
    task
}

/// `apps:update`. A stored item only takes a new name (its target is its
/// copy in the file store). A custom icon the user picked is kept unless
/// they pick another or ask for auto-detect.
pub fn update_app(
    apps: &mut [Entry],
    id: &str,
    form: &AppForm,
    resolve: impl FnOnce(&str) -> Option<Shortcut>,
) -> Option<IconTask> {
    let entry = &mut apps[find(apps, id)?];
    if !form.name.is_empty() {
        entry.set("name", form.name.as_str());
    }
    if entry.str("type") == Some("stored") {
        return None;
    }
    entry.set("type", form.type_.as_str());
    entry.set("target", form.target.as_str());
    entry.set("args", form.args.as_str());
    entry.set("cwd", form.cwd.as_str());
    entry.set("interpreter", form.interpreter.as_str());
    entry.set("command", form.command.as_str());
    apply_shortcut(entry, resolve);

    if !form.custom_icon_path.is_empty() {
        entry.set("iconIsCustom", true);
        return Some(IconTask::SaveCustom { id: id.to_owned(), source: form.custom_icon_path.clone() });
    }
    if form.force_auto_icon || !entry.truthy("iconIsCustom") {
        entry.set("iconIsCustom", false);
        if entry.truthy("target") {
            return Some(icon_task(id, entry));
        }
    }
    None
}

/// `apps:remove`. An island's contents aren't removed with it: they move up
/// to where the island lived, on fresh cells. `None` when no entry had `id`
/// (1.2.2 still deleted icons\<id>.png then; skipped so an id that names
/// nothing can't name a file either).
pub fn remove(apps: &mut Vec<Entry>, id: &str) -> Option<Removed> {
    let target = find(apps, id).map(|i| apps[i].clone());
    if let Some(island) = target.as_ref().filter(|t| t.str("kind") == Some("island")) {
        let up = island.get("parentId").cloned().unwrap_or(Value::Null);
        for a in apps.iter_mut().filter(|a| a.str("parentId") == Some(id)) {
            a.set("parentId", up.clone());
            // Its old cell was only valid inside the removed island.
            a.remove("gridX");
            a.remove("gridY");
        }
    }
    apps.retain(|a| a.str("id") != Some(id));
    store::assign_missing_positions(apps);
    target.map(|t| Removed { stored: t.str("type") == Some("stored") })
}

/// `folder:add`: a new, collapsed island on the first free cell.
pub fn add_island(apps: &mut Vec<Entry>, id: String, name: &str, parent_id: &str, color: &str, now: i64) {
    let mut entry = Entry::from_value(json!({
        "id": id,
        "kind": "island",
        "name": if name.is_empty() { "New Island" } else { name },
        "parentId": null_if_empty(parent_id),
        "color": null_if_empty(color),
        "pinned": false,
        "collapsed": true,
        "w": js_num(layout::EXPANDED_ISLAND_W),
        "h": js_num(layout::EXPANDED_ISLAND_H),
        "createdAt": now,
    }))
    .expect("an object");
    layout::place_free(apps, &mut entry, None);
    apps.push(entry);
}

/// `item:rename`: trimmed; a blank name is ignored.
pub fn rename(apps: &mut [Entry], id: &str, name: &str) {
    let name = js_trim(name);
    if let (Some(i), false) = (find(apps, id), name.is_empty()) {
        apps[i].set("name", name);
    }
}

/// `item:setColor`: `""` clears it.
pub fn set_color(apps: &mut [Entry], id: &str, color: &str) {
    if let Some(i) = find(apps, id) {
        apps[i].set("color", null_if_empty(color));
    }
}

/// `item:move` into `parent_id` (`""` = Home), landing on the free cell
/// nearest `cell` (the drop point in the new parent) or, without one, on
/// the first free cell if it has none. Moving an island into itself or one
/// of its own islands does nothing.
pub fn move_item(apps: &mut [Entry], id: &str, parent_id: &str, cell: Option<(f64, f64)>) {
    let Some(i) = find(apps, id) else { return };
    if apps[i].str("kind") == Some("island")
        && !parent_id.is_empty()
        && descendant_island_ids(apps, id).contains(parent_id)
    {
        return; // would create a cycle
    }
    if parent_id == id {
        return;
    }
    let new_parent = null_if_empty(parent_id);
    let entry = &mut apps[i];
    if !strict_eq(parent_key(entry), Some(&new_parent).filter(|v| !v.is_null())) {
        // Old cells were only valid in the previous parent's coordinate space.
        entry.remove("gridX");
        entry.remove("gridY");
    }
    entry.set("parentId", new_parent);
    match cell.filter(|(x, y)| x.is_finite() && y.is_finite()) {
        Some(cell) => {
            layout::place_free_at(apps, i, Some(cell));
        }
        None if apps[i].num("gridX").is_none() => {
            layout::place_free_at(apps, i, None);
        }
        None => {}
    }
}

// `Number(x) || 0` for a number the UI already made.
fn or_zero(n: f64) -> f64 {
    if n.is_nan() {
        0.0
    } else {
        n
    }
}

/// `item:setPosition`: the free cell nearest the one asked for.
pub fn set_position(apps: &mut [Entry], id: &str, x: f64, y: f64) {
    if let Some(i) = find(apps, id) {
        layout::place_free_at(apps, i, Some((or_zero(x), or_zero(y))));
    }
}

/// `item:setPositions`: a dragged multi-selection, placed in order so later
/// items avoid the cells earlier ones just took.
pub fn set_positions(apps: &mut [Entry], updates: &[(&str, f64, f64)]) {
    for &(id, x, y) in updates {
        set_position(apps, id, x, y);
    }
}

// Back on the nearest free cell after an island's footprint changed size.
fn replace_island(apps: &mut [Entry], i: usize) {
    let at = |key| apps[i].num(key).unwrap_or(0.0);
    let cell = (at("gridX"), at("gridY"));
    layout::place_free_at(apps, i, Some(cell));
}

fn find_island(apps: &[Entry], id: &str) -> Option<usize> {
    find(apps, id).filter(|&i| apps[i].str("kind") == Some("island"))
}

/// `item:setCollapsed` (islands only).
pub fn set_collapsed(apps: &mut [Entry], id: &str, collapsed: bool) {
    if let Some(i) = find_island(apps, id) {
        apps[i].set("collapsed", collapsed);
        replace_island(apps, i);
    }
}

/// `item:setShowNames` (islands only).
pub fn set_show_names(apps: &mut [Entry], id: &str, show_names: bool) {
    if let Some(i) = find_island(apps, id) {
        apps[i].set("showNames", show_names);
    }
}

/// `item:setSize` (islands only): rounded and kept within layout's
/// MIN/MAX_ISLAND_*; an expanded island moves if it now overlaps something.
pub fn set_size(apps: &mut [Entry], id: &str, w: f64, h: f64) {
    let Some(i) = find_island(apps, id) else { return };
    // Math.max(min, Math.min(max, NaN)) is NaN, which JSON writes as null.
    let clamp = |n: f64, min, max| if n.is_nan() { n } else { js_round(n).clamp(min, max) };
    apps[i].set_num("w", clamp(w, layout::MIN_ISLAND_W, layout::MAX_ISLAND_W));
    apps[i].set_num("h", clamp(h, layout::MIN_ISLAND_H, layout::MAX_ISLAND_H));
    if apps[i].get("collapsed") == Some(&Value::Bool(false)) {
        replace_island(apps, i);
    }
}

fn pin(apps: &mut [Entry], i: usize, pinned: bool) {
    let was_pinned = apps[i].truthy("pinned");
    apps[i].set("pinned", pinned);
    if pinned && !was_pinned {
        // Counts this entry's own old pinOrder too, as 1.2.2 does.
        let order = store::next_pin_order(apps);
        apps[i].set_num("pinOrder", order);
    }
}

/// `item:togglePin`: a newly pinned item goes to the end of the taskbar.
pub fn toggle_pin(apps: &mut [Entry], id: &str) {
    if let Some(i) = find(apps, id) {
        let pinned = !apps[i].truthy("pinned");
        pin(apps, i, pinned);
    }
}

/// `item:setPinned`: for bulk actions, where a mixed selection has to end
/// up in one state. Pinning an already pinned item keeps its place.
pub fn set_pinned(apps: &mut [Entry], id: &str, pinned: bool) {
    if let Some(i) = find(apps, id) {
        pin(apps, i, pinned);
    }
}

/// `item:setPinOrder`: `ordered_ids` is the whole taskbar in its new order.
pub fn set_pin_order(apps: &mut [Entry], ordered_ids: &[&str]) {
    for (n, id) in ordered_ids.iter().enumerate() {
        if let Some(i) = find(apps, id) {
            apps[i].set("pinOrder", n);
        }
    }
}

/// `apps:launch`'s checks: the entry to hand to `win::launch::launch_item`.
pub fn launchable<'a>(apps: &'a [Entry], id: &str) -> Result<&'a Entry> {
    let entry = apps.iter().find(|a| a.str("id") == Some(id)).ok_or_else(|| Error::msg("App not found"))?;
    if entry.str("kind") == Some("island") {
        return Err(Error::msg("That is an island, not an app"));
    }
    Ok(entry)
}

/// `launchAndRecord`, after the launch succeeded (reload the list first:
/// 1.2.2 recorded on a fresh load). Feeds Recent.
pub fn record_launch(apps: &mut [Entry], id: &str, now: i64) {
    let Some(i) = find(apps, id) else { return };
    apps[i].set("lastLaunched", now);
    // shortcut: a non-number launchCount counts from 0; 1.2.2 would have
    // concatenated a string one. Only a hand-edited file has those.
    let count = apps[i].num("launchCount").unwrap_or(0.0) + 1.0;
    apps[i].set_num("launchCount", count);
}

/// `apps:detectMissingIcons`: icons to fetch for apps whose icon file is
/// missing. `fixed` in 1.2.2's reply is how many `apply_icon` calls return true.
pub fn detect_missing_icons(apps: &[Entry], icon_exists: impl Fn(&str) -> bool) -> Vec<IconTask> {
    apps.iter()
        .filter(|e| e.str("kind") == Some("app") && e.truthy("target") && e.str("type") != Some("stored"))
        .filter(|e| !e.str("iconPath").is_some_and(|p| !p.is_empty() && icon_exists(p)))
        .filter_map(|e| Some(icon_task(e.str("id")?, e)))
        .collect()
}

// `fetchDefaultIcons`: only File Explorer's; the Recycle Bin's is drawn.
// 1.2.2 also skipped a target that isn't on disk; here the extraction fails
// for it instead, with the same result.
fn default_icon_task(entry: &Entry) -> Option<IconTask> {
    if entry.truthy("special") || entry.str("type") != Some("path") || !entry.truthy("target") {
        return None;
    }
    Some(icon_task(entry.str("id")?, entry))
}

/// The first run's apps (File Explorer, Recycle Bin) and their icons.
pub fn first_run(new_id: impl FnMut() -> String, now: i64) -> (Vec<Entry>, Vec<IconTask>) {
    let entries = defaults::build_entries(new_id, now);
    let icons = entries.iter().filter_map(default_icon_task).collect();
    (entries, icons)
}

/// `apps:restoreDefaults`: adds the stock apps whose name isn't in use,
/// each on the free cell nearest its first-run spot.
pub fn restore_defaults(apps: &mut Vec<Entry>, new_id: impl FnMut() -> String, now: i64) -> DefaultsChange {
    // A non-string name can't equal a stock one, so "" stands in for it.
    let names: HashSet<String> = apps.iter().map(|a| a.str("name").unwrap_or("").to_lowercase()).collect();
    let mut change = DefaultsChange::default();
    for mut entry in defaults::build_entries(new_id, now) {
        if names.contains(&entry.str("name").unwrap_or("").to_lowercase()) {
            continue;
        }
        let cell = entry.num("gridX").zip(entry.num("gridY"));
        layout::place_free(apps, &mut entry, cell);
        change.icons.extend(default_icon_task(&entry));
        change.added += 1;
        apps.push(entry);
    }
    change
}

/// `apps:repairDefaults` (see `defaults::repair`), plus fresh icons for the
/// repaired ones without a custom icon and for the added ones.
pub fn repair_defaults(apps: &mut Vec<Entry>, new_id: impl FnMut() -> String, now: i64) -> DefaultsChange {
    let r = defaults::repair(apps, new_id, now);
    let fresh = r.repaired.iter().filter(|&&i| !apps[i].truthy("iconIsCustom")).chain(&r.added);
    let icons = fresh.filter_map(|&i| default_icon_task(&apps[i])).collect();
    DefaultsChange { repaired: r.repaired.len(), added: r.added.len(), icons }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(v: Value) -> Entry {
        Entry::from_value(v).unwrap()
    }

    fn list(vs: Vec<Value>) -> Vec<Entry> {
        vs.into_iter().map(e).collect()
    }

    fn get<'a>(apps: &'a [Entry], id: &str) -> &'a Entry {
        &apps[find(apps, id).unwrap()]
    }

    fn cell(apps: &[Entry], id: &str) -> (f64, f64) {
        let a = get(apps, id);
        (a.num("gridX").unwrap(), a.num("gridY").unwrap())
    }

    fn app(id: &str, parent: Value, x: i64, y: i64) -> Value {
        json!({ "id": id, "kind": "app", "name": id, "parentId": parent, "gridX": x, "gridY": y })
    }

    fn island(id: &str, parent: Value, x: i64, y: i64) -> Value {
        json!({ "id": id, "kind": "island", "name": id, "parentId": parent, "collapsed": true, "w": 3, "h": 2, "gridX": x, "gridY": y })
    }

    fn form(type_: &str, target: &str) -> AppForm {
        AppForm { name: "N".into(), type_: type_.into(), target: target.into(), ..Default::default() }
    }

    fn no_lnk(_: &str) -> Option<Shortcut> {
        panic!("not a shortcut")
    }

    fn counter() -> impl FnMut() -> String {
        let mut n = 0;
        move || {
            n += 1;
            format!("00000000-0000-4000-8000-{n:012}")
        }
    }

    #[test]
    fn validate_form_uses_the_dialogs_messages() {
        let msg = |f: &AppForm| validate_form(f).unwrap_err().to_string();
        assert_eq!(msg(&AppForm::default()), "Give it a name.");
        assert_eq!(msg(&form("path", "")), "Set a target.");
        assert_eq!(msg(&form("command", "")), "Enter a command.");
        assert!(validate_form(&AppForm { command: "dir".into(), ..form("command", "") }).is_ok());
        assert!(validate_form(&form("url", "https://x")).is_ok());
    }

    #[test]
    fn shortcut_detection_follows_node_extname() {
        assert!(is_shortcut("C:\\a\\App.LNK"));
        assert!(is_shortcut("..lnk"));
        assert!(!is_shortcut(".lnk"));
        assert!(!is_shortcut("C:\\x.lnk\\file"));
    }

    #[test]
    fn apply_shortcut_takes_target_trimmed_args_and_a_cwd_that_isnt_the_targets_folder() {
        let mut entry = e(json!({ "type": "path", "target": "C:\\s\\a.lnk", "args": "", "cwd": "" }));
        apply_shortcut(&mut entry, |p| {
            assert_eq!(p, "C:\\s\\a.lnk");
            Some(Shortcut { target: "C:\\App\\a.exe".into(), args: "  -x ".into(), cwd: "C:/App/".into() })
        });
        assert_eq!(entry.str("target"), Some("C:\\App\\a.exe"));
        assert_eq!(entry.str("args"), Some("-x"));
        assert_eq!(entry.str("cwd"), Some("")); // same folder, written differently
        apply_shortcut(&mut entry, no_lnk); // now an .exe: not read again

        let mut entry = e(json!({ "type": "path", "target": "C:\\s\\a.lnk", "args": "keep", "cwd": "" }));
        apply_shortcut(&mut entry, |_| Some(Shortcut { target: "C:\\a.exe".into(), args: " ".into(), cwd: "D:\\w".into() }));
        assert_eq!((entry.str("args"), entry.str("cwd")), (Some("keep"), Some("D:\\w")));

        let mut unreadable = e(json!({ "type": "path", "target": "C:\\s\\a.lnk" }));
        apply_shortcut(&mut unreadable, |_| None);
        assert_eq!(unreadable.str("target"), Some("C:\\s\\a.lnk"));
        let mut url = e(json!({ "type": "url", "target": "x.lnk" }));
        apply_shortcut(&mut url, no_lnk);
    }

    #[test]
    fn resolve_parts_matches_path_win32_resolve() {
        let s = |p| {
            let (root, segs) = resolve_parts(p);
            format!("{root}{}", segs.join("\\"))
        };
        assert_eq!(s("C:\\x\\.\\y\\..\\z"), "C:\\x\\z");
        assert_eq!(s("C:/x/y/"), "C:\\x\\y");
        assert_eq!(s("C:\\a\\..\\..\\b"), "C:\\b");
        assert_eq!(s("C:\\a\\\\b\\"), "C:\\a\\b");
        assert_eq!(s("\\\\srv\\share\\app.exe"), "\\\\srv\\share\\app.exe");
    }

    #[test]
    fn infer_type_from_ext_picks_script_interpreters() {
        assert_eq!(infer_type_from_ext("C:\\a.PY"), ("script", "python"));
        assert_eq!(infer_type_from_ext("x.mjs"), ("script", "node"));
        assert_eq!(infer_type_from_ext("x.js"), ("script", "node"));
        assert_eq!(infer_type_from_ext("x.exe"), ("path", ""));
    }

    #[test]
    fn descendant_island_ids_goes_all_the_way_down_and_skips_apps() {
        let apps = list(vec![
            island("a", Value::Null, 0, 0),
            island("b", json!("a"), 0, 0),
            island("c", json!("b"), 0, 0),
            app("x", json!("a"), 1, 0),
            island("d", Value::Null, 1, 0),
        ]);
        let mut ids: Vec<_> = descendant_island_ids(&apps, "a").into_iter().collect();
        ids.sort();
        assert_eq!(ids, ["a", "b", "c"]);
    }

    #[test]
    fn add_app_builds_the_1_2_2_entry_on_a_free_cell_and_asks_for_its_icon() {
        let mut apps = list(vec![app("old", Value::Null, 0, 0)]);
        let task = add_app(&mut apps, &form("url", "https://a.b"), "n1".into(), 7, no_lnk);
        assert_eq!(task, Some(IconTask::Favicon { id: "n1".into(), url: "https://a.b".into() }));
        assert_eq!(
            serde_json::to_string(&apps[1]).unwrap(),
            r#"{"id":"n1","kind":"app","name":"N","type":"url","target":"https://a.b","args":"","cwd":"","interpreter":"","command":"","parentId":null,"pinned":false,"createdAt":7,"lastLaunched":null,"launchCount":0,"color":null,"iconPath":null,"iconIsCustom":false,"gridX":1,"gridY":0}"#
        );
        assert!(apply_icon(&mut apps, &task.unwrap(), Some("C:\\i\\n1.png".into())));
        assert_eq!(apps[1].str("iconPath"), Some("C:\\i\\n1.png"));

        let custom = AppForm { custom_icon_path: "C:\\pic.png".into(), parent_id: "isl".into(), ..form("path", "C:\\a.exe") };
        let task = add_app(&mut apps, &custom, "n2".into(), 8, |_| None).unwrap();
        assert_eq!(task, IconTask::SaveCustom { id: "n2".into(), source: "C:\\pic.png".into() });
        assert_eq!(get(&apps, "n2").get("iconIsCustom"), Some(&json!(true)));
        assert_eq!(get(&apps, "n2").str("parentId"), Some("isl"));
        assert_eq!(cell(&apps, "n2"), (0.0, 0.0)); // its own island's space

        let blank = AppForm { type_: "command".into(), command: "dir".into(), ..Default::default() };
        assert_eq!(add_app(&mut apps, &blank, "n3".into(), 9, no_lnk), None);
        assert_eq!(get(&apps, "n3").str("name"), Some("Untitled"));
    }

    #[test]
    fn add_app_resolves_a_shortcut_before_fetching_its_icon() {
        let mut apps = Vec::new();
        let lnk = |_: &str| Some(Shortcut { target: "C:\\P\\p.exe".into(), ..Default::default() });
        let task = add_app(&mut apps, &form("path", "C:\\d\\p.lnk"), "n".into(), 1, lnk);
        assert_eq!(task, Some(IconTask::Extract { id: "n".into(), path: "C:\\P\\p.exe".into() }));
    }

    #[test]
    fn update_app_keeps_a_custom_icon_unless_asked_and_stored_items_only_rename() {
        let mut apps = list(vec![
            json!({ "id": "a", "kind": "app", "name": "A", "type": "path", "target": "C:\\a.exe", "iconPath": "C:\\i\\a.png", "iconIsCustom": true }),
            json!({ "id": "s", "kind": "app", "name": "S", "type": "stored", "target": "C:\\files\\s\\f.txt" }),
        ]);
        let f = AppForm { name: String::new(), ..form("path", "C:\\b.exe") };
        assert_eq!(update_app(&mut apps, "a", &f, no_lnk), None);
        assert_eq!((apps[0].str("name"), apps[0].str("target")), (Some("A"), Some("C:\\b.exe")));
        assert_eq!(apps[0].get("iconIsCustom"), Some(&json!(true)));

        let task = update_app(&mut apps, "a", &AppForm { force_auto_icon: true, ..f.clone() }, no_lnk).unwrap();
        assert_eq!(apps[0].get("iconIsCustom"), Some(&json!(false)));
        assert!(!apply_icon(&mut apps, &task, None)); // nothing found: the old icon stays
        assert_eq!(apps[0].str("iconPath"), Some("C:\\i\\a.png"));

        let picked = AppForm { custom_icon_path: "C:\\p.png".into(), ..f };
        let task = update_app(&mut apps, "a", &picked, no_lnk).unwrap();
        assert!(apply_icon(&mut apps, &task, Some("C:\\i\\a.png".into())));
        assert_eq!(apps[0].get("iconIsCustom"), Some(&json!(true)));

        assert_eq!(update_app(&mut apps, "s", &form("url", "https://x"), no_lnk), None);
        assert_eq!((apps[1].str("name"), apps[1].str("type")), (Some("N"), Some("stored")));
        assert_eq!(update_app(&mut apps, "nope", &form("url", "x"), no_lnk), None);
    }

    #[test]
    fn removing_an_island_moves_its_contents_up_onto_free_cells() {
        let mut apps = list(vec![
            app("home", Value::Null, 0, 0),
            island("isl", Value::Null, 1, 0),
            app("in", json!("isl"), 0, 0),
            island("sub", json!("isl"), 1, 0),
            app("deep", json!("sub"), 0, 0),
        ]);
        assert_eq!(remove(&mut apps, "isl"), Some(Removed { stored: false }));
        assert_eq!(find(&apps, "isl"), None);
        assert_eq!(get(&apps, "in").get("parentId"), Some(&Value::Null));
        assert_eq!(get(&apps, "sub").get("parentId"), Some(&Value::Null));
        assert_eq!(get(&apps, "deep").str("parentId"), Some("sub")); // only direct children move
        let mut home: Vec<_> = ["home", "in", "sub"].iter().map(|id| cell(&apps, id)).collect();
        home.sort_by(|a, b| a.partial_cmp(b).unwrap());
        home.dedup();
        assert_eq!(home.len(), 3);
    }

    #[test]
    fn removing_a_stored_item_asks_for_its_copy_to_go_and_an_unknown_id_asks_nothing() {
        let mut apps = list(vec![json!({ "id": "s", "kind": "app", "type": "stored", "parentId": null, "gridX": 0, "gridY": 0 })]);
        assert_eq!(remove(&mut apps, "zz"), None);
        assert_eq!(remove(&mut apps, "s"), Some(Removed { stored: true }));
        assert!(apps.is_empty());
    }

    #[test]
    fn add_island_is_collapsed_with_the_expanded_default_size() {
        let mut apps = list(vec![app("a", Value::Null, 0, 0)]);
        add_island(&mut apps, "i".into(), "", "", "#fff", 5);
        assert_eq!(
            serde_json::to_string(&apps[1]).unwrap(),
            r##"{"id":"i","kind":"island","name":"New Island","parentId":null,"color":"#fff","pinned":false,"collapsed":true,"w":3,"h":2,"createdAt":5,"gridX":1,"gridY":0}"##
        );
    }

    #[test]
    fn rename_trims_and_ignores_blank_and_set_color_clears_with_empty() {
        let mut apps = list(vec![json!({ "id": "a", "name": "A", "color": "#000" })]);
        rename(&mut apps, "a", "  \u{FEFF} ");
        assert_eq!(apps[0].str("name"), Some("A"));
        rename(&mut apps, "a", "  B ");
        assert_eq!(apps[0].str("name"), Some("B"));
        set_color(&mut apps, "a", "");
        assert_eq!(apps[0].get("color"), Some(&Value::Null));
        set_color(&mut apps, "a", "#123");
        assert_eq!(apps[0].str("color"), Some("#123"));
    }

    #[test]
    fn move_item_refuses_cycles_and_places_fresh_in_the_new_parent() {
        let mut apps = list(vec![
            island("a", Value::Null, 0, 0),
            island("b", json!("a"), 0, 0),
            app("x", Value::Null, 5, 5),
            app("y", json!("a"), 1, 0),
        ]);
        let before = apps.clone();
        move_item(&mut apps, "a", "b", None);
        move_item(&mut apps, "a", "a", None);
        move_item(&mut apps, "x", "x", None);
        assert_eq!(apps, before);

        move_item(&mut apps, "x", "a", None);
        assert_eq!(get(&apps, "x").str("parentId"), Some("a"));
        assert_eq!(cell(&apps, "x"), (2.0, 0.0)); // first free among a's (b at 0,0, y at 1,0)

        move_item(&mut apps, "x", "", Some((1.0, 0.0)));
        assert_eq!(get(&apps, "x").get("parentId"), Some(&Value::Null));
        assert_eq!(cell(&apps, "x"), (1.0, 0.0)); // next to island a at Home

        move_item(&mut apps, "x", "", Some((0.0, 0.0)));
        assert_eq!(cell(&apps, "x"), (1.0, 0.0)); // a is there: nearest free

        move_item(&mut apps, "x", "", None);
        assert_eq!(cell(&apps, "x"), (1.0, 0.0)); // same parent, no cell: stays
    }

    #[test]
    fn set_position_and_set_positions_avoid_each_other() {
        let mut apps = list(vec![app("a", Value::Null, 0, 0), app("b", Value::Null, 1, 0), app("c", Value::Null, 2, 0)]);
        set_position(&mut apps, "a", f64::NAN, 3.0);
        assert_eq!(cell(&apps, "a"), (0.0, 3.0));
        set_positions(&mut apps, &[("b", 5.0, 5.0), ("c", 5.0, 5.0)]);
        assert_eq!(cell(&apps, "b"), (5.0, 5.0));
        assert_ne!(cell(&apps, "c"), (5.0, 5.0));
    }

    #[test]
    fn set_collapsed_moves_an_expanding_island_off_its_neighbour() {
        let mut apps = list(vec![island("i", Value::Null, 0, 0), app("a", Value::Null, 1, 0)]);
        set_collapsed(&mut apps, "a", false); // not an island
        assert_eq!(get(&apps, "a").get("collapsed"), None);
        set_collapsed(&mut apps, "i", false);
        assert_eq!(get(&apps, "i").get("collapsed"), Some(&json!(false)));
        assert_ne!(cell(&apps, "i"), (0.0, 0.0));
    }

    #[test]
    fn set_show_names_only_for_islands() {
        let mut apps = list(vec![island("i", Value::Null, 0, 0), app("a", Value::Null, 1, 0)]);
        set_show_names(&mut apps, "i", true);
        set_show_names(&mut apps, "a", true);
        assert_eq!(get(&apps, "i").get("showNames"), Some(&json!(true)));
        assert_eq!(get(&apps, "a").get("showNames"), None);
    }

    #[test]
    fn set_size_rounds_and_clamps_and_replaces_only_expanded_islands() {
        let mut apps = list(vec![island("i", Value::Null, 0, 0), app("a", Value::Null, 2, 0)]);
        set_size(&mut apps, "i", 99.0, 0.4);
        assert_eq!((get(&apps, "i").get("w"), get(&apps, "i").get("h")), (Some(&json!(8)), Some(&json!(2))));
        assert_eq!(cell(&apps, "i"), (0.0, 0.0)); // collapsed: 1x1, nothing to move
        set_collapsed(&mut apps, "i", false);
        let at = cell(&apps, "i");
        set_size(&mut apps, "i", 2.5, 2.0);
        assert_eq!(get(&apps, "i").get("w"), Some(&json!(3)));
        assert_ne!(at, (0.0, 0.0));
        set_size(&mut apps, "i", f64::NAN, 2.0);
        assert_eq!(get(&apps, "i").get("w"), Some(&Value::Null));
    }

    #[test]
    fn pinning_appends_to_the_taskbar_and_counts_an_old_pin_order() {
        let mut apps = list(vec![
            json!({ "id": "a", "pinned": true, "pinOrder": 0 }),
            json!({ "id": "b", "pinned": false, "pinOrder": 4 }),
            json!({ "id": "c", "pinned": false }),
        ]);
        toggle_pin(&mut apps, "b");
        assert_eq!(apps[1].get("pinOrder"), Some(&json!(5))); // its own stale 4 counted, as 1.2.2 does
        toggle_pin(&mut apps, "b");
        assert_eq!(apps[1].get("pinned"), Some(&json!(false)));
        set_pinned(&mut apps, "a", true);
        assert_eq!(apps[0].get("pinOrder"), Some(&json!(0))); // already pinned: keeps its place
        set_pinned(&mut apps, "c", true);
        assert_eq!(apps[2].get("pinOrder"), Some(&json!(1)));
        set_pinned(&mut apps, "c", false);
        assert_eq!(apps[2].get("pinned"), Some(&json!(false)));
    }

    #[test]
    fn set_pin_order_numbers_in_the_given_order() {
        let mut apps = list(vec![json!({ "id": "a", "pinOrder": 0 }), json!({ "id": "b", "pinOrder": 1 })]);
        set_pin_order(&mut apps, &["b", "gone", "a"]);
        assert_eq!((apps[0].get("pinOrder"), apps[1].get("pinOrder")), (Some(&json!(2)), Some(&json!(0))));
    }

    #[test]
    fn launching_checks_the_entry_and_records_it_for_recent() {
        let mut apps = list(vec![app("a", Value::Null, 0, 0), island("i", Value::Null, 1, 0)]);
        assert_eq!(launchable(&apps, "x").unwrap_err().to_string(), "App not found");
        assert_eq!(launchable(&apps, "i").unwrap_err().to_string(), "That is an island, not an app");
        assert!(launchable(&apps, "a").is_ok());
        record_launch(&mut apps, "a", 100);
        record_launch(&mut apps, "a", 200);
        assert_eq!((apps[0].get("lastLaunched"), apps[0].get("launchCount")), (Some(&json!(200)), Some(&json!(2))));
    }

    #[test]
    fn detect_missing_icons_skips_stored_islands_and_present_icons_and_clears_custom() {
        let mut apps = list(vec![
            json!({ "id": "a", "kind": "app", "type": "path", "target": "C:\\a.exe", "iconPath": "C:\\i\\a.png", "iconIsCustom": true }),
            json!({ "id": "b", "kind": "app", "type": "url", "target": "https://b", "iconPath": "C:\\i\\b.png" }),
            json!({ "id": "c", "kind": "app", "type": "path", "target": "" }),
            json!({ "id": "s", "kind": "app", "type": "stored", "target": "C:\\s" }),
            json!({ "id": "i", "kind": "island", "target": "C:\\x" }),
        ]);
        let tasks = detect_missing_icons(&apps, |p| p == "C:\\i\\b.png");
        assert_eq!(tasks, [IconTask::Extract { id: "a".into(), path: "C:\\a.exe".into() }]);
        assert!(apply_icon(&mut apps, &tasks[0], Some("C:\\i\\a2.png".into())));
        assert_eq!(apps[0].get("iconIsCustom"), Some(&json!(false)));
    }

    #[test]
    fn first_run_fetches_only_file_explorers_icon() {
        let (entries, icons) = first_run(counter(), 1);
        assert_eq!(entries.len(), 2);
        assert_eq!(icons, [IconTask::Extract { id: entries[0].str("id").unwrap().into(), path: defaults::EXPLORER_PATH.into() }]);
    }

    #[test]
    fn restore_defaults_adds_only_missing_names_near_their_first_run_cell() {
        let mut apps = list(vec![json!({ "id": "x", "kind": "app", "name": "file explorer", "parentId": null, "gridX": 1, "gridY": 0 })]);
        let change = restore_defaults(&mut apps, counter(), 1);
        assert_eq!((change.added, change.icons.len()), (1, 0)); // the Recycle Bin, whose icon is drawn
        assert_eq!(apps[1].str("name"), Some("Recycle Bin"));
        assert_ne!(cell(&apps, apps[1].str("id").unwrap()), (1.0, 0.0));
        assert_eq!(restore_defaults(&mut apps, counter(), 2).added, 0);
    }

    #[test]
    fn repair_defaults_refetches_icons_unless_custom() {
        let (mut apps, _) = first_run(counter(), 1);
        apps[0].set("iconIsCustom", true);
        let change = repair_defaults(&mut apps, counter(), 2);
        assert_eq!(change, DefaultsChange { repaired: 2, added: 0, icons: vec![] });
        apps[0].set("iconIsCustom", false);
        apps.remove(1);
        let change = repair_defaults(&mut apps, counter(), 3);
        assert_eq!((change.repaired, change.added, change.icons.len()), (1, 1, 1));
    }
}
