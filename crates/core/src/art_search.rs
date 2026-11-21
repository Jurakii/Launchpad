//! Finding pictures online: app icons (Edit App → Search Icons) and Media
//! art. Three sources, all public APIs: Wikimedia Commons (no key; logos of
//! most well-known apps, as PNG thumbnails), SteamGridDB (games' icons,
//! covers, banners and logos; needs the user's free API key) and
//! ScreenScraper (console and PC games' box art, fanart and logos; needs a
//! developer login, plus the user's account for better limits).
use serde_json::Value;

/// What the app supplies: a GET returning the body when the status is 2xx.
pub trait SearchHttp {
    fn get_json(&mut self, url: &str, headers: &str) -> Option<Value>;
}

/// One picture found: `url` is what's downloaded when it's picked.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub thumb: String,
    pub url: String,
}

const SGDB: &str = "https://www.steamgriddb.com/api/v2";

fn bearer(key: &str) -> String {
    format!("Authorization: Bearer {}", key.trim())
}

/// Commons files matching `query`, as 256 px PNG/JPEG thumbnails.
pub fn commons(query: &str, http: &mut impl SearchHttp) -> Vec<Hit> {
    let q = crate::favicon::encode_uri_component(&format!("{} logo", query.trim()));
    let url = format!(
        "https://commons.wikimedia.org/w/api.php?action=query&format=json&generator=search&gsrnamespace=6&gsrlimit=16&gsrsearch={q}&prop=imageinfo&iiprop=url|mime&iiurlwidth=256"
    );
    let Some(doc) = http.get_json(&url, "") else { return vec![] };
    let mut pages: Vec<&Value> = doc.pointer("/query/pages").and_then(Value::as_object).map(|o| o.values().collect()).unwrap_or_default();
    // The search's own order.
    pages.sort_by_key(|p| p.get("index").and_then(Value::as_i64).unwrap_or(i64::MAX));
    pages
        .into_iter()
        .filter_map(|p| {
            let info = p.pointer("/imageinfo/0")?;
            let mime = info.get("mime").and_then(Value::as_str).unwrap_or("");
            if !(mime.starts_with("image/png") || mime.starts_with("image/jpeg") || mime.starts_with("image/svg")) {
                return None;
            }
            let thumb = info.get("thumburl").and_then(Value::as_str)?.to_string();
            Some(Hit { url: thumb.clone(), thumb })
        })
        .collect()
}

/// SteamGridDB's best match for a game name.
pub fn sgdb_game(name: &str, key: &str, http: &mut impl SearchHttp) -> Option<u64> {
    let url = format!("{SGDB}/search/autocomplete/{}", crate::favicon::encode_uri_component(name.trim()));
    http.get_json(&url, &bearer(key))?.pointer("/data/0/id")?.as_u64()
}

/// A game's pictures of one kind on SteamGridDB: "icons", "grids" (covers,
/// 600x900 only), "heroes" (banners) or "logos". PNG and JPEG only.
pub fn sgdb_assets(game: u64, kind: &str, key: &str, http: &mut impl SearchHttp) -> Vec<Hit> {
    let filter = match kind {
        "grids" => "?dimensions=600x900&mimes=image/png,image/jpeg",
        "icons" => "?mimes=image/png",
        _ => "?mimes=image/png,image/jpeg",
    };
    let url = format!("{SGDB}/{kind}/game/{game}{filter}");
    let Some(doc) = http.get_json(&url, &bearer(key)) else { return vec![] };
    doc.get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|a| {
            let url = a.get("url").and_then(Value::as_str)?.to_string();
            let thumb = a.get("thumb").and_then(Value::as_str).unwrap_or(&url).to_string();
            Some(Hit { thumb, url })
        })
        .collect()
}

/// Icons for an app: SteamGridDB's (with a key), then Commons'.
pub fn icons(query: &str, sgdb_key: Option<&str>, http: &mut impl SearchHttp) -> Vec<Hit> {
    let mut hits = Vec::new();
    if let Some(key) = sgdb_key.filter(|k| !k.trim().is_empty()) {
        if let Some(game) = sgdb_game(query, key, http) {
            hits.extend(sgdb_assets(game, "icons", key, http).into_iter().take(12));
        }
    }
    hits.extend(commons(query, http));
    hits.truncate(24);
    hits
}

/// What ScreenScraper asks for on every call. `dev_id`/`dev_password`
/// identify the program; `user`/`password` are the user's own account
/// (optional, but it raises their daily quota and gets them in when the
/// site is busy).
#[derive(Debug, Clone, Default)]
pub struct SsLogin {
    pub dev_id: String,
    pub dev_password: String,
    pub user: String,
    pub password: String,
}

const SS: &str = "https://api.screenscraper.fr/api2";

impl SsLogin {
    fn query(&self) -> String {
        let e = crate::favicon::encode_uri_component;
        let mut q = format!("devid={}&devpassword={}&softname=launchpad&output=json", e(&self.dev_id), e(&self.dev_password));
        if !self.user.is_empty() && !self.password.is_empty() {
            q += &format!("&ssid={}&sspassword={}", e(&self.user), e(&self.password));
        }
        q
    }
}

/// ScreenScraper's system id for a Media platform id; Windows PC for games
/// with none.
pub fn ss_system(platform: Option<&str>) -> Option<u32> {
    Some(match platform {
        None => 138,
        Some(p) => match p {
            "nes" => 3,
            "snes" => 4,
            "n64" => 14,
            "gb" => 9,
            "gbc" => 10,
            "gba" => 12,
            "nds" => 15,
            "genesis" => 1,
            "mastersystem" => 2,
            "psx" => 57,
            "ps2" => 58,
            "psp" => 61,
            "gamecube" => 13,
            "wii" => 16,
            "saturn" => 22,
            "dreamcast" => 23,
            "arcade" => 75,
            _ => return None,
        },
    })
}

/// The game's ScreenScraper record: by its ROM's file name and size when
/// there is one (how the site identifies dumps), else by name.
pub fn ss_game(login: &SsLogin, name: &str, rom: Option<(&str, u64)>, system: Option<u32>, http: &mut impl SearchHttp) -> Option<Value> {
    let e = crate::favicon::encode_uri_component;
    let sys = system.map(|s| format!("&systemeid={s}")).unwrap_or_default();
    let game = match rom {
        Some((file, size)) => {
            let url = format!("{SS}/jeuInfos.php?{}{sys}&romtype=rom&romnom={}&romtaille={size}", login.query(), e(file));
            http.get_json(&url, "")?.pointer("/response/jeu")?.clone()
        }
        None => {
            let url = format!("{SS}/jeuRecherche.php?{}{sys}&recherche={}", login.query(), e(name.trim()));
            http.get_json(&url, "")?.pointer("/response/jeux/0")?.clone()
        }
    };
    // A search with no match answers with one empty game.
    game.get("id").is_some().then_some(game)
}

/// Picture addresses from a ScreenScraper record for the slots in `want`:
/// cover = box front, banner = fanart, logo = wheel. World or US art first.
pub fn ss_art(game: &Value, want: &[&'static str]) -> Vec<(&'static str, String)> {
    const REGIONS: [&str; 6] = ["wor", "us", "ss", "eu", "uk", "jp"];
    let medias: &[Value] = game.get("medias").and_then(Value::as_array).map_or(&[], Vec::as_slice);
    let rank = |m: &Value| {
        let r = m.get("region").and_then(Value::as_str).unwrap_or("");
        REGIONS.iter().position(|x| *x == r).unwrap_or(REGIONS.len())
    };
    let best = |types: &[&str]| {
        types.iter().find_map(|t| {
            medias
                .iter()
                .filter(|m| m.get("type").and_then(Value::as_str) == Some(t))
                .min_by_key(|m| rank(m))
                .and_then(|m| m.get("url").and_then(Value::as_str))
                .map(str::to_string)
        })
    };
    [("cover", &["box-2D"][..]), ("banner", &["fanart", "ss"][..]), ("logo", &["wheel-hd", "wheel"][..])]
        .into_iter()
        .filter(|(slot, _)| want.contains(slot))
        .filter_map(|(slot, types)| Some((slot, best(types)?)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Fake {
        replies: HashMap<String, Value>,
        asked: Vec<(String, String)>,
    }

    impl SearchHttp for Fake {
        fn get_json(&mut self, url: &str, headers: &str) -> Option<Value> {
            self.asked.push((url.to_string(), headers.to_string()));
            self.replies.iter().find(|(k, _)| url.starts_with(k.as_str())).map(|(_, v)| v.clone())
        }
    }

    #[test]
    fn commons_keeps_bitmaps_and_drawings_in_search_order() {
        let mut http = Fake::default();
        http.replies.insert(
            "https://commons.wikimedia.org/".into(),
            json!({ "query": { "pages": {
                "2": { "index": 2, "imageinfo": [{ "thumburl": "https://t/b.png", "mime": "image/png" }] },
                "1": { "index": 1, "imageinfo": [{ "thumburl": "https://t/a.svg.png", "mime": "image/svg+xml" }] },
                "3": { "index": 3, "imageinfo": [{ "thumburl": "https://t/c.pdf.jpg", "mime": "application/pdf" }] },
            }}}),
        );
        let hits = commons("Notion", &mut http);
        assert_eq!(hits.iter().map(|h| h.url.as_str()).collect::<Vec<_>>(), ["https://t/a.svg.png", "https://t/b.png"]);
        assert!(http.asked[0].0.contains("gsrsearch=Notion%20logo"));
    }

    #[test]
    fn screenscraper_looks_up_roms_by_file_and_picks_world_art() {
        let login = SsLogin { dev_id: "d".into(), dev_password: "p w".into(), user: "u".into(), password: "x".into() };
        let mut http = Fake::default();
        http.replies.insert(
            format!("{SS}/jeuInfos.php"),
            json!({ "response": { "jeu": { "id": "3", "medias": [
                { "type": "box-2D", "region": "jp", "url": "https://s/jp.png" },
                { "type": "box-2D", "region": "us", "url": "https://s/us.png" },
                { "type": "wheel", "region": "wor", "url": "https://s/wheel.png" },
                { "type": "wheel-hd", "region": "eu", "url": "https://s/wheelhd.png" },
            ]}}}),
        );
        let game = ss_game(&login, "Sonic", Some(("Sonic 2 (World).zip", 749652)), ss_system(Some("genesis")), &mut http).unwrap();
        let url = &http.asked[0].0;
        assert!(url.contains("devpassword=p%20w") && url.contains("ssid=u") && url.contains("systemeid=1"), "{url}");
        assert!(url.contains("romnom=Sonic%202%20(World).zip&romtaille=749652"), "{url}");
        assert_eq!(ss_art(&game, &["cover", "banner", "logo"]), vec![("cover", "https://s/us.png".to_string()), ("logo", "https://s/wheelhd.png".to_string())]);
        // A name search with no match: nothing.
        let mut http = Fake::default();
        http.replies.insert(format!("{SS}/jeuRecherche.php"), json!({ "response": { "jeux": [{}] } }));
        assert!(ss_game(&SsLogin::default(), "Nope", None, ss_system(None), &mut http).is_none());
        assert!(http.asked[0].0.contains("systemeid=138&recherche=Nope") && !http.asked[0].0.contains("ssid"));
    }

    #[test]
    fn icons_ask_steamgriddb_first_with_the_key_then_commons() {
        let mut http = Fake::default();
        http.replies.insert(format!("{SGDB}/search/autocomplete/"), json!({ "data": [{ "id": 42 }] }));
        http.replies.insert(format!("{SGDB}/icons/game/42"), json!({ "data": [{ "url": "https://s/i.png", "thumb": "https://s/t.png" }] }));
        http.replies.insert("https://commons.wikimedia.org/".into(), json!({}));
        let hits = icons("Terraria", Some("abc"), &mut http);
        assert_eq!(hits, vec![Hit { thumb: "https://s/t.png".into(), url: "https://s/i.png".into() }]);
        assert_eq!(http.asked[0].1, "Authorization: Bearer abc");
        // No key: Commons only.
        let mut http = Fake::default();
        icons("Terraria", None, &mut http);
        assert_eq!(http.asked.len(), 1);
    }
}
