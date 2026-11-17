//! Updating and "Reinstall this version" (Settings → Repair), one path for
//! both: download a release's installer from its v<version> tag, check it
//! against that release's latest.yml, and hand back its path for the app to
//! run. An update is the same thing aimed at the newest release's version
//! instead of the running one. HTTP and SHA-512 are passed in.
use crate::favicon::{encode_uri_component, is_js_space};
use crate::{Error, Result};
use std::path::{Path, PathBuf};

pub const REPO: &str = "Jurakii/Launchpad";

pub fn release_base(version: &str) -> Result<String> {
    let (core, pre) = match version.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (version, None),
    };
    let numbers: Vec<&str> = core.split('.').collect();
    let valid = numbers.len() == 3
        && numbers.iter().all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        && pre.is_none_or(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-'));
    if !valid {
        return Err(Error::msg(format!("\"{version}\" isn't a release version.")));
    }
    Ok(format!("https://github.com/{REPO}/releases/download/v{version}/"))
}

/// Where the newest release's latest.yml is (GitHub redirects to its tag).
pub fn latest_yml_url() -> String {
    format!("https://github.com/{REPO}/releases/latest/download/latest.yml")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatestYml {
    /// The installer's file name.
    pub file: String,
    /// Its SHA-512, base64.
    pub sha512: String,
    pub version: Option<String>,
}

/// First line `name: value` at the start of a line, the value without its
/// quotes. Walks the text the way the JS regex
/// `^name:\s*['"]?([^'"\r\n]+?)['"]?\s*$` (multiline) did, so the odd cases
/// come out the same: an empty value takes the next line's text.
fn field(text: &str, name: &str) -> Option<String> {
    let c: Vec<char> = text.chars().collect();
    let eol = |i: usize| i == c.len() || matches!(c[i], '\n' | '\r' | '\u{2028}' | '\u{2029}');
    let quote = |i: usize| matches!(c.get(i), Some('\'' | '"'));
    // Only whitespace between `i` and the end of a line.
    let ends_line = |mut i: usize| loop {
        if eol(i) {
            return true;
        }
        if !is_js_space(c[i]) {
            return false;
        }
        i += 1;
    };
    let key: Vec<char> = format!("{name}:").chars().collect();
    for line in 0..c.len() {
        if !(line == 0 || eol(line - 1)) || !c[line..].starts_with(&key) {
            continue;
        }
        let after = line + key.len();
        let spaces = c[after..].iter().take_while(|&&ch| is_js_space(ch)).count();
        for skipped in (0..=spaces).rev() {
            let at = after + skipped;
            for start in [at + 1, at].into_iter().skip(usize::from(!quote(at))) {
                let mut end = start;
                while end < c.len() && !matches!(c[end], '\'' | '"' | '\r' | '\n') {
                    end += 1;
                    if ends_line(end) || (quote(end) && ends_line(end + 1)) {
                        return Some(c[start..end].iter().collect());
                    }
                }
            }
        }
    }
    None
}

/// The fields electron-builder writes at the top level of latest.yml.
/// Not a YAML parser: the file is machine-written and only these are needed.
pub fn parse_latest_yml(text: &str) -> Result<LatestYml> {
    let (Some(file), Some(sha512)) = (field(text, "path"), field(text, "sha512")) else {
        return Err(Error::msg("The release's latest.yml has no installer listed."));
    };
    // The name is joined onto the release URL and a temp folder: no path
    // parts (what Node's path.basename would strip: folders, a drive letter).
    let b = file.as_bytes();
    let has_path_parts = file.contains(['/', '\\']) || (b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':');
    if has_path_parts || !file.to_ascii_lowercase().ends_with(".exe") {
        return Err(Error::msg("The release's installer name isn't usable."));
    }
    Ok(LatestYml { file, sha512, version: field(text, "version") })
}

// ---- Versions ----

#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum PreId {
    // Order matters: numeric identifiers sort below alphanumeric ones.
    Number(u64),
    Text(String),
}

/// major.minor.patch and the prerelease identifiers.
type Version = ([u64; 3], Vec<PreId>);

fn number(text: &str) -> Option<u64> {
    let plain = !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) && (text == "0" || !text.starts_with('0'));
    // JS numbers stop being exact past 2^53 - 1, where semver stops too.
    text.parse().ok().filter(|&n| plain && n <= 9_007_199_254_740_991)
}

/// Strict semver (as the `semver` package's `parse`): `1.2.3`, `1.2.3-beta.1`,
/// an optional leading `v`, build metadata accepted and ignored.
fn parse_version(text: &str) -> Option<Version> {
    let text = text.trim_matches(is_js_space);
    let text = text.strip_prefix('v').unwrap_or(text);
    let identifiers = |s: &str| s.split('.').all(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'));
    let (text, build) = match text.split_once('+') {
        Some((t, b)) => (t, Some(b)),
        None => (text, None),
    };
    if text.len() > 256 || !build.is_none_or(identifiers) {
        return None;
    }
    let (core, pre) = match text.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (text, None),
    };
    let mut parts = core.split('.').map(number);
    let numbers = [parts.next()??, parts.next()??, parts.next()??];
    if parts.next().is_some() || !pre.is_none_or(identifiers) {
        return None;
    }
    let mut ids = Vec::new();
    for id in pre.into_iter().flat_map(|p| p.split('.')) {
        ids.push(if id.bytes().all(|b| b.is_ascii_digit()) { PreId::Number(number(id)?) } else { PreId::Text(id.to_string()) });
    }
    Some((numbers, ids))
}

/// Whether `latest` is a newer version than `current`, by semver precedence
/// as electron-updater compared them: 1.2.10 > 1.2.9, and 2.0.0 is newer
/// than 2.0.0-beta.2, which is newer than 2.0.0-beta.1 and 1.9.9.
pub fn is_newer(latest: &str, current: &str) -> Result<bool> {
    let parse = |v: &str| parse_version(v).ok_or_else(|| Error::msg(format!("\"{v}\" isn't a release version.")));
    let (latest, current) = (parse(latest)?, parse(current)?);
    Ok(match latest.0.cmp(&current.0) {
        std::cmp::Ordering::Equal => match (latest.1.is_empty(), current.1.is_empty()) {
            (true, false) => true,
            (false, false) => latest.1 > current.1,
            _ => false,
        },
        order => order.is_gt(),
    })
}

// ---- Downloading ----

pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

fn fetch_ok(fetch: &mut impl FnMut(&str) -> Result<HttpResponse>, url: &str) -> Result<Vec<u8>> {
    let res = fetch(url)?;
    if !(200..=299).contains(&res.status) {
        return Err(Error::msg(if res.status == 404 {
            "This version's release isn't on GitHub (a dev build, or the release was removed).".to_string()
        } else {
            format!("Download failed ({}).", res.status)
        }));
    }
    Ok(res.body)
}

/// The version of the newest release if it is newer than `current_version`.
/// `fetch` is a GET that follows redirects; its error is passed on.
pub fn check_for_update(current_version: &str, mut fetch: impl FnMut(&str) -> Result<HttpResponse>) -> Result<Option<String>> {
    let res = fetch(&latest_yml_url())?;
    if !(200..=299).contains(&res.status) {
        return Err(Error::msg(format!("Download failed ({}).", res.status)));
    }
    let info = parse_latest_yml(&String::from_utf8_lossy(&res.body))?;
    let Some(version) = info.version else {
        return Err(Error::msg("The release's latest.yml doesn't say which version it is."));
    };
    Ok(is_newer(&version, current_version)?.then_some(version))
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().fold(0u32, |n, &b| n << 8 | u32::from(b)) << (8 * (3 - chunk.len()));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Refuses `installer` unless its SHA-512 is the one latest.yml lists
/// (`expected`, base64 as written there).
pub fn verify_installer(installer: &[u8], expected: &str, sha512: impl FnOnce(&[u8]) -> [u8; 64]) -> Result<()> {
    if base64(&sha512(installer)) != expected {
        return Err(Error::msg("The downloaded installer doesn't match the release's checksum. Try again."));
    }
    Ok(())
}

/// Returns the downloaded, verified installer's path in `temp_dir`.
/// `fetch` is a GET that follows redirects; its error is passed on.
pub fn download(
    version: &str,
    mut fetch: impl FnMut(&str) -> Result<HttpResponse>,
    sha512: impl FnOnce(&[u8]) -> [u8; 64],
    temp_dir: &Path,
) -> Result<PathBuf> {
    let base = release_base(version)?;
    let yml = fetch_ok(&mut fetch, &format!("{base}latest.yml"))?;
    let info = parse_latest_yml(&String::from_utf8_lossy(&yml))?;
    if let Some(theirs) = info.version.as_deref().filter(|v| *v != version) {
        return Err(Error::msg(format!("That release is for version {theirs}.")));
    }
    let installer = fetch_ok(&mut fetch, &format!("{base}{}", encode_uri_component(&info.file)))?;
    verify_installer(&installer, &info.sha512, sha512)?;
    std::fs::create_dir_all(temp_dir)?;
    let dest = temp_dir.join(&info.file);
    std::fs::write(&dest, installer)?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::temp_dir;
    use std::collections::HashMap;

    fn yml(sha: &str) -> String {
        [
            "version: 1.0.4",
            "files:",
            "  - url: Launchpad-Setup-1.0.4.exe",
            &format!("    sha512: {sha}"),
            "    size: 7",
            "path: Launchpad-Setup-1.0.4.exe",
            &format!("sha512: {sha}"),
            "releaseDate: '2026-01-01T00:00:00.000Z'",
            "",
        ]
        .join("\n")
    }

    fn message<T: std::fmt::Debug>(result: Result<T>) -> String {
        match result {
            Err(Error::Msg(m)) => m,
            other => panic!("expected a message, got {other:?}"),
        }
    }

    /// Stands in for SHA-512: 64 bytes that depend on every input byte.
    fn fake_sha512(data: &[u8]) -> [u8; 64] {
        let mut out = [0u8; 64];
        for (i, b) in data.iter().enumerate() {
            out[i % 64] = out[i % 64].wrapping_mul(31).wrapping_add(*b);
            out[(i + 7) % 64] ^= b.rotate_left(3);
        }
        out
    }

    /// Serves `files` by the last part of the URL, 404 for anything else.
    fn fake_fetch<'a>(files: &'a HashMap<&str, Vec<u8>>, urls: &'a mut Vec<String>) -> impl FnMut(&str) -> Result<HttpResponse> + 'a {
        move |url| {
            urls.push(url.to_string());
            Ok(match files.get(url.rsplit('/').next().unwrap()) {
                Some(body) => HttpResponse { status: 200, body: body.clone() },
                None => HttpResponse { status: 404, body: Vec::new() },
            })
        }
    }

    #[test]
    fn release_base_pins_the_tag_to_the_running_version() {
        assert_eq!(release_base("1.0.4").unwrap(), "https://github.com/Jurakii/Launchpad/releases/download/v1.0.4/");
        assert!(release_base("../latest").is_err());
    }

    #[test]
    fn parse_latest_yml_reads_the_installer_and_refuses_names_with_path_parts() {
        assert_eq!(
            parse_latest_yml(&yml("abc=")).unwrap(),
            LatestYml { file: "Launchpad-Setup-1.0.4.exe".into(), sha512: "abc=".into(), version: Some("1.0.4".into()) }
        );
        assert!(parse_latest_yml("path: ..\\evil.exe\nsha512: x").is_err());
        assert!(parse_latest_yml("version: 1").is_err());
    }

    #[test]
    fn download_fetches_from_v_version_checks_the_checksum_and_saves_the_installer() {
        let exe = b"MZfake!";
        let sha = base64(&fake_sha512(exe));
        let files = HashMap::from([("latest.yml", yml(&sha).into_bytes()), ("Launchpad-Setup-1.0.4.exe", exe.to_vec())]);
        let mut urls = Vec::new();
        let dir = temp_dir();
        let temp = dir.path().join("launchpad-reinstall");
        let dest = download("1.0.4", fake_fetch(&files, &mut urls), fake_sha512, &temp).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), exe);
        assert_eq!(dest, temp.join("Launchpad-Setup-1.0.4.exe"));
        assert_eq!(urls.len(), 2);
        assert!(urls.iter().all(|u| u.contains("/download/v1.0.4/")));
    }

    #[test]
    fn download_refuses_a_mismatched_checksum_and_reports_a_missing_release() {
        let dir = temp_dir();
        let files = HashMap::from([("latest.yml", yml("nope").into_bytes()), ("Launchpad-Setup-1.0.4.exe", b"MZ".to_vec())]);
        let result = download("1.0.4", fake_fetch(&files, &mut Vec::new()), fake_sha512, dir.path());
        assert_eq!(message(result), "The downloaded installer doesn't match the release's checksum. Try again.");
        assert!(crate::test_util::list_dir(dir.path()).is_empty());

        let missing = HashMap::new();
        let result = download("1.0.4", fake_fetch(&missing, &mut Vec::new()), fake_sha512, dir.path());
        assert_eq!(message(result), "This version's release isn't on GitHub (a dev build, or the release was removed).");
    }

    // --- Beyond the JS tests ---

    #[test]
    fn release_base_takes_what_the_js_pattern_took() {
        for version in ["0.0.0", "10.20.30", "2.0.0-beta.1", "2.0.0-rc-1.x", "01.02.03", "1.2.3--"] {
            assert!(release_base(version).is_ok(), "{version}");
        }
        for version in ["", "1.2", "1.2.3.4", "v1.2.3", "1.2.3-", "1.2.3+build", "1.2.3-a_b", "1.2.x", " 1.2.3", "1.2.3\n", "1..3", "1.2.3/..", "١.٢.٣"] {
            assert_eq!(message(release_base(version)), format!("\"{version}\" isn't a release version."));
        }
    }

    #[test]
    fn latest_yml_fields_in_their_written_forms() {
        let parsed = parse_latest_yml("version: '2.0.0'\r\npath: \"Launchpad Setup.EXE\"  \r\nsha512:   q+/=\r\n").unwrap();
        assert_eq!(parsed, LatestYml { file: "Launchpad Setup.EXE".into(), sha512: "q+/=".into(), version: Some("2.0.0".into()) });
        // Indented keys (the per-file entries) are not the top-level ones.
        assert_eq!(message(parse_latest_yml("files:\n  - path: a.exe\n    sha512: x\n")), "The release's latest.yml has no installer listed.");
        assert_eq!(message(parse_latest_yml("path: a.exe\n")), "The release's latest.yml has no installer listed.");
        assert_eq!(message(parse_latest_yml("")), "The release's latest.yml has no installer listed.");
        assert_eq!(parse_latest_yml("sha512: x\npath: a.exe").unwrap().version, None);
        // The first of two wins.
        assert_eq!(parse_latest_yml("path: a.exe\npath: b.exe\nsha512: x").unwrap().file, "a.exe");
    }

    #[test]
    fn field_matches_like_the_js_regex() {
        // Each expectation is what the JS regex returned for the same text.
        let cases: [(&str, Option<&str>); 20] = [
            ("path: a.exe", Some("a.exe")),
            ("path:a.exe", Some("a.exe")),
            ("path:\ta.exe \t", Some("a.exe")),
            ("path: 'a.exe'", Some("a.exe")),
            ("path: 'a.exe", Some("a.exe")),
            ("path: a.exe'", Some("a.exe")),
            ("path: \"a b.exe\" ", Some("a b.exe")),
            ("path: a'b.exe", None),
            ("path: ''", None),
            ("path:", None),
            ("path: ", Some(" ")),
            ("path:\nsha512: x", Some("sha512: x")),
            ("path:   \n\n  next.exe\n", Some("next.exe")),
            (" path: a.exe", None),
            ("xpath: a.exe\rpath: b.exe\u{2028}", Some("b.exe")),
            ("path: a.exe # note", Some("a.exe # note")),
            ("path: 'a.exe\"", Some("a.exe")),
            ("path:  ' a.exe ' ", Some(" a.exe ")),
            ("path: a.exe\u{a0}", Some("a.exe")),
            ("path: \u{2028}x.exe", Some("x.exe")),
        ];
        for (text, expected) in cases {
            assert_eq!(field(text, "path").as_deref(), expected, "{text:?}");
        }
    }

    #[test]
    fn installer_names_that_are_refused() {
        for name in ["../evil.exe", "..\\evil.exe", "dir/setup.exe", "C:setup.exe", "c:\\setup.exe", "/setup.exe", "setup.msi", "setup.exe.txt", "setup", ".."] {
            let text = format!("path: {name}\nsha512: x");
            assert_eq!(message(parse_latest_yml(&text)), "The release's installer name isn't usable.", "{name}");
        }
        for name in ["Launchpad-Setup-2.0.0.exe", "setup.EXE", "Launchpad Setup.exe", ".exe"] {
            assert_eq!(parse_latest_yml(&format!("path: {name}\nsha512: x")).unwrap().file, name);
        }
    }

    #[test]
    fn base64_matches_rfc_4648() {
        let cases = [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")];
        for (text, expected) in cases {
            assert_eq!(base64(text.as_bytes()), expected);
        }
        assert_eq!(base64(&[0xfb, 0xff, 0xfe]), "+//+");
        assert_eq!(base64(&[0u8; 64]).len(), 88);
    }

    #[test]
    fn verify_installer_compares_with_the_base64_digest() {
        // SHA-512 of "MZfake!" and its base64, from Node's crypto.
        const DIGEST: [u8; 64] = [
            34, 23, 175, 179, 218, 163, 136, 83, 114, 138, 137, 97, 52, 55, 105, 107, 147, 176, 193, 53, 234, 6, 7, 181, 65, 61,
            95, 202, 75, 69, 150, 162, 123, 220, 28, 129, 228, 245, 125, 126, 165, 148, 157, 190, 10, 120, 51, 48, 137, 239, 61,
            200, 170, 114, 81, 23, 216, 82, 36, 53, 233, 176, 161, 175,
        ];
        let expected = "Ihevs9qjiFNyiolhNDdpa5OwwTXqBge1QT1fyktFlqJ73ByB5PV9fqWUnb4KeDMwie89yKpyURfYUiQ16bChrw==";
        let digest = |data: &[u8]| {
            assert_eq!(data, b"MZfake!");
            DIGEST
        };
        assert!(verify_installer(b"MZfake!", expected, digest).is_ok());
        // Exact text only, as the JS compared strings.
        assert!(verify_installer(b"MZfake!", &expected.to_lowercase(), digest).is_err());
        assert!(verify_installer(b"MZfake!", expected.trim_end_matches('='), digest).is_err());
        assert!(verify_installer(b"MZfake!", "", digest).is_err());
    }

    #[test]
    fn download_refuses_a_release_for_another_version() {
        let dir = temp_dir();
        let files = HashMap::from([("latest.yml", yml("x").into_bytes())]);
        let mut urls = Vec::new();
        let result = download("1.0.5", fake_fetch(&files, &mut urls), fake_sha512, dir.path());
        assert_eq!(message(result), "That release is for version 1.0.4.");
        assert_eq!(urls, ["https://github.com/Jurakii/Launchpad/releases/download/v1.0.5/latest.yml"]);

        let mut urls = Vec::new();
        let result = download("latest", fake_fetch(&files, &mut urls), fake_sha512, dir.path());
        assert_eq!(message(result), "\"latest\" isn't a release version.");
        assert!(urls.is_empty());
    }

    #[test]
    fn download_reports_other_failures_and_passes_fetch_errors_on() {
        let dir = temp_dir();
        let result = download("1.0.4", |_: &str| Ok(HttpResponse { status: 503, body: Vec::new() }), fake_sha512, dir.path());
        assert_eq!(message(result), "Download failed (503).");
        let result = download("1.0.4", |_: &str| Err(Error::msg("net::ERR_INTERNET_DISCONNECTED")), fake_sha512, dir.path());
        assert_eq!(message(result), "net::ERR_INTERNET_DISCONNECTED");

        // The installer itself missing from the release.
        let sha = base64(&fake_sha512(b"MZ"));
        let files = HashMap::from([("latest.yml", yml(&sha).into_bytes())]);
        let result = download("1.0.4", fake_fetch(&files, &mut Vec::new()), fake_sha512, dir.path());
        assert_eq!(message(result), "This version's release isn't on GitHub (a dev build, or the release was removed).");
    }

    #[test]
    fn download_escapes_the_installer_name_in_the_url() {
        let exe = b"MZ";
        let text = format!("path: Launchpad Setup #1.exe\nsha512: {}\n", base64(&fake_sha512(exe)));
        let files = HashMap::from([("latest.yml", text.into_bytes()), ("Launchpad%20Setup%20%231.exe", exe.to_vec())]);
        let mut urls = Vec::new();
        let dir = temp_dir();
        let dest = download("2.0.0", fake_fetch(&files, &mut urls), fake_sha512, dir.path()).unwrap();
        assert_eq!(dest, dir.path().join("Launchpad Setup #1.exe"));
        assert_eq!(urls[1], "https://github.com/Jurakii/Launchpad/releases/download/v2.0.0/Launchpad%20Setup%20%231.exe");
    }

    #[test]
    fn is_newer_follows_semver_precedence() {
        let newer = [
            ("1.2.3", "1.2.2"),
            ("1.2.10", "1.2.9"),
            ("1.10.0", "1.9.9"),
            ("2.0.0", "1.99.99"),
            ("2.0.0", "2.0.0-beta.2"),
            ("2.0.0-beta.2", "2.0.0-beta.1"),
            ("2.0.0-beta.10", "2.0.0-beta.9"),
            ("2.0.0-beta.1", "2.0.0-beta"),
            ("2.0.0-beta", "2.0.0-alpha.9"),
            ("2.0.0-alpha", "2.0.0-9"),
            ("2.0.0-rc.1", "2.0.0-beta.11"),
            ("2.0.0-beta", "2.0.0-Beta"),
            ("2.0.0-beta.1", "1.2.2"),
            ("v1.2.3", "1.2.2"),
        ];
        for (latest, current) in newer {
            assert!(is_newer(latest, current).unwrap(), "{latest} > {current}");
            assert!(!is_newer(current, latest).unwrap(), "{current} < {latest}");
        }
        for (a, b) in [("1.2.3", "1.2.3"), ("1.2.3", " v1.2.3 "), ("1.2.3+a", "1.2.3+b"), ("2.0.0-beta.1", "2.0.0-beta.1")] {
            assert!(!is_newer(a, b).unwrap() && !is_newer(b, a).unwrap(), "{a} = {b}");
        }
        for bad in ["", "1.2", "1.2.3.4", "01.2.3", "1.2.3-", "1.2.3-01", "1.2.3-a..b", "1.2.3+", "x.y.z", "1.2.3-ä", "99999999999999999.0.0"] {
            assert_eq!(message(is_newer(bad, "1.0.0")), format!("\"{bad}\" isn't a release version."));
            assert!(is_newer("1.0.0", bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn check_for_update_reports_only_newer_releases() {
        let files = HashMap::from([("latest.yml", yml("x").into_bytes())]);
        let mut urls = Vec::new();
        assert_eq!(check_for_update("1.0.3", fake_fetch(&files, &mut urls)).unwrap().as_deref(), Some("1.0.4"));
        assert_eq!(urls, ["https://github.com/Jurakii/Launchpad/releases/latest/download/latest.yml"]);
        assert_eq!(check_for_update("1.0.4", fake_fetch(&files, &mut Vec::new())).unwrap(), None);
        assert_eq!(check_for_update("1.0.5", fake_fetch(&files, &mut Vec::new())).unwrap(), None);
        assert_eq!(check_for_update("1.0.4-beta.1", fake_fetch(&files, &mut Vec::new())).unwrap().as_deref(), Some("1.0.4"));

        let none = HashMap::new();
        assert_eq!(message(check_for_update("1.0.3", fake_fetch(&none, &mut Vec::new()))), "Download failed (404).");
        let unversioned = HashMap::from([("latest.yml", b"path: a.exe\nsha512: x\n".to_vec())]);
        assert_eq!(
            message(check_for_update("1.0.3", fake_fetch(&unversioned, &mut Vec::new()))),
            "The release's latest.yml doesn't say which version it is."
        );
        assert!(check_for_update("1.0.3", |_: &str| Err(Error::msg("offline"))).is_err());
    }
}
