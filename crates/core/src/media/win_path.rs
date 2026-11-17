//! The pieces of Node's `path.win32` the media modules used, on strings, so
//! they behave the same wherever the tests run.

fn is_sep(b: u8) -> bool {
    b == b'/' || b == b'\\'
}

/// `path.win32.isAbsolute`.
pub fn is_absolute(path: &str) -> bool {
    let b = path.as_bytes();
    match b.first() {
        None => false,
        Some(&c) if is_sep(c) => true,
        Some(c) => b.len() > 2 && c.is_ascii_alphabetic() && b[1] == b':' && is_sep(b[2]),
    }
}

/// `path.win32.basename(path)`: the last part, ignoring trailing separators
/// and a leading drive letter.
pub fn basename(path: &str) -> &str {
    let b = path.as_bytes();
    let start = if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' { 2 } else { 0 };
    let rest = path[start..].trim_end_matches(['/', '\\']);
    match rest.rfind(['/', '\\']) {
        Some(i) => &rest[i + 1..],
        None => rest,
    }
}

/// `path.win32.extname`: from the last dot of the last part; a leading dot
/// (`.bashrc`) and `..` have no extension.
pub fn extname(path: &str) -> &str {
    let name = basename(path);
    match name.rfind('.') {
        None | Some(0) => "",
        Some(_) if name == ".." => "",
        Some(dot) => &name[dot..],
    }
}

/// `path.win32.basename(path, path.win32.extname(path))`.
pub fn stem(path: &str) -> &str {
    let name = basename(path);
    &name[..name.len() - extname(path).len()]
}

/// `path.win32.dirname`, following Node's implementation step for step.
pub fn dirname(path: &str) -> &str {
    let b = path.as_bytes();
    let len = b.len();
    if len == 0 {
        return ".";
    }
    if len == 1 {
        return if is_sep(b[0]) { path } else { "." };
    }
    let mut root_end = None;
    let mut offset = 0;
    if is_sep(b[0]) {
        // Possible UNC root
        root_end = Some(1);
        offset = 1;
        if is_sep(b[1]) {
            let mut j = 2;
            let mut last = j;
            while j < len && !is_sep(b[j]) {
                j += 1;
            }
            if j < len && j != last {
                last = j;
                while j < len && is_sep(b[j]) {
                    j += 1;
                }
                if j < len && j != last {
                    last = j;
                    while j < len && !is_sep(b[j]) {
                        j += 1;
                    }
                    if j == len {
                        return path; // a UNC root only
                    }
                    if j != last {
                        root_end = Some(j + 1);
                        offset = j + 1;
                    }
                }
            }
        }
    } else if b[0].is_ascii_alphabetic() && b[1] == b':' {
        let end = if len > 2 && is_sep(b[2]) { 3 } else { 2 };
        root_end = Some(end);
        offset = end;
    }
    let mut end = None;
    let mut matched_slash = true;
    for i in (offset..len).rev() {
        if is_sep(b[i]) {
            if !matched_slash {
                end = Some(i);
                break;
            }
        } else {
            matched_slash = false;
        }
    }
    match end.or(root_end) {
        Some(end) => &path[..end],
        None => ".",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Expected values are what node's path.win32 returns.
    #[test]
    fn matches_node() {
        assert_eq!(dirname("D:\\Shows\\S01\\Pilot.mkv"), "D:\\Shows\\S01");
        assert_eq!(dirname("C:\\a.sfc"), "C:\\");
        assert_eq!(dirname("C:\\"), "C:\\");
        assert_eq!(dirname("C:a"), "C:");
        assert_eq!(dirname("a\\b\\"), "a");
        assert_eq!(dirname("snes"), ".");
        assert_eq!(dirname("\\\\server\\share\\dir\\f.mkv"), "\\\\server\\share\\dir");
        assert_eq!(dirname("\\\\server\\share\\f.mkv"), "\\\\server\\share\\");
        assert_eq!(dirname("\\\\server\\share"), "\\\\server\\share");
        assert_eq!(dirname("/a/b"), "/a");
        assert_eq!(dirname("\\a"), "\\");

        assert_eq!(extname("a.B"), ".B");
        assert_eq!(extname(".bashrc"), "");
        assert_eq!(extname(".."), "");
        assert_eq!(extname("..."), ".");
        assert_eq!(extname("..a"), ".a");
        assert_eq!(extname("a."), ".");
        assert_eq!(extname("C:\\x.y\\file"), "");
        assert_eq!(extname("C:\\x\\f.tar.gz\\"), ".gz");
        assert_eq!(stem("C:\\Games\\Celeste\\Celeste.exe"), "Celeste");
        assert_eq!(stem("(USA).sfc"), "(USA)");
        assert_eq!(stem("C:foo.txt"), "foo");
        assert_eq!(basename("C:\\"), "");

        assert!(is_absolute("C:\\x") && is_absolute("c:/x") && is_absolute("\\x") && is_absolute("/x"));
        assert!(!is_absolute("C:") && !is_absolute("C:x") && !is_absolute("x.exe") && !is_absolute(""));
    }
}
