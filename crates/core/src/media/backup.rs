//! Media's part of a Launchpad backup (version 3): the library entries
//! and the player and emulator profiles. Art is left out - it can be large
//! and can be set or scraped again - and so are scraper keys.
use serde_json::{json, Value};

use super::library::Library;
use super::players;
use crate::Result;

/// `{ library, profiles }` for the backup file.
pub fn export_data(library: &mut Library) -> Result<Value> {
    let entries = library.load()?;
    let profiles = players::load(library.dir())?;
    Ok(json!({ "library": entries, "profiles": profiles }))
}

/// Replaces the library and profiles with a backup's. A backup without them
/// (version 1 or 2) leaves both alone. Returns how many entries came in, or
/// None when there was nothing to import.
pub fn import_data(library: &mut Library, bundle: &Value) -> Result<Option<usize>> {
    let Some(bundle) = bundle.as_object() else { return Ok(None) };
    let mut count = None;
    if let Some(Value::Array(list)) = bundle.get("library") {
        count = Some(library.replace_all(list.clone())?.len());
    }
    // typeof 'object': a list counts too, and sanitize() turns it into empty data.
    if let Some(profiles @ (Value::Object(_) | Value::Array(_))) = bundle.get("profiles") {
        players::save(library.dir(), profiles)?;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::library::{is_valid_id, normalize};
    use crate::test_util::{temp_dir, TempDir};

    const ID: &str = "11111111-1111-4111-8111-111111111111";

    fn profile() -> Value {
        json!({ "id": "mpv", "name": "mpv", "kind": "player", "exe": "C:\\mpv\\mpv.exe", "args": ["{file}"], "platforms": ["videos"] })
    }

    fn setup() -> (TempDir, Library) {
        let dir = temp_dir();
        let library = Library::new(dir.path());
        (dir, library)
    }

    fn obj(v: Value) -> serde_json::Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn export_carries_the_library_and_profiles_and_import_puts_them_back() {
        let (_a_dir, mut a) = setup();
        a.save(&[normalize(obj(json!({ "id": ID, "library": "videos", "name": "Arrival", "target": "D:\\Arrival.mkv", "media": { "cover": { "path": "cover.jpg", "source": "user" } } })))]).unwrap();
        players::save(a.dir(), &json!({ "profiles": [profile()], "defaults": { "videos": "mpv" } })).unwrap();
        let mut data = export_data(&mut a).unwrap();
        data["version"] = 3.into();

        let (_b_dir, mut b) = setup();
        assert_eq!(import_data(&mut b, &data).unwrap(), Some(1));
        let entries = b.load().unwrap();
        assert_eq!(entries[0].id(), ID, "ids survive, so art folders still match");
        assert_eq!(entries[0].0["media"]["cover"], json!({ "path": "cover.jpg", "source": "user" }));
        let profiles = serde_json::to_value(players::load(b.dir()).unwrap()).unwrap();
        assert_eq!(profiles, json!({ "profiles": [profile()], "defaults": { "videos": "mpv" } }));
    }

    #[test]
    fn older_backups_leave_the_library_and_profiles_alone() {
        let (_dir, mut library) = setup();
        library.save(&[normalize(obj(json!({ "id": ID, "name": "Keep", "target": "D:\\k.sfc" })))]).unwrap();
        players::save(library.dir(), &json!({ "profiles": [profile()], "defaults": {} })).unwrap();
        assert_eq!(import_data(&mut library, &json!({ "version": 2, "apps": [], "layouts": {} })).unwrap(), None);
        assert_eq!(import_data(&mut library, &json!([{ "name": "v1 entry" }])).unwrap(), None);
        assert_eq!(library.load().unwrap()[0].name(), "Keep");
        assert_eq!(players::load(library.dir()).unwrap().profiles.len(), 1);
    }

    #[test]
    fn import_cleans_what_it_is_given() {
        let (_dir, mut library) = setup();
        let bundle = json!({ "library": [null, { "id": "../evil", "name": "X", "library": "music" }], "profiles": { "profiles": [{ "id": "bad" }] } });
        import_data(&mut library, &bundle).unwrap();
        let entries = library.load().unwrap();
        assert!(is_valid_id(entries[0].id()));
        assert_eq!(entries[0].library(), "games");
    }
}
