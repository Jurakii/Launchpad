//! Finding a website's own icon: URL and `<link>` parsing, and the order
//! in which places are tried. No network or image decoding here - both come
//! in through [`IconSource`].

/// JS `\s` (and what `String.prototype.trim` strips).
pub(crate) fn is_js_space(c: char) -> bool {
    c == '\u{feff}' || (c.is_whitespace() && c != '\u{85}')
}

/// JS `encodeURIComponent`.
pub fn encode_uri_component(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

// ---- URLs ----

// Characters the URL standard percent-encodes in each part, on top of
// controls, space and everything non-ASCII.
const PATH_SET: &str = "\"#<>?^`{}";
const QUERY_SET: &str = "\"#<>'";
const FRAGMENT_SET: &str = "\"<>`";

fn percent_encode(text: &str, set: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b <= 0x20 || b >= 0x7f || set.as_bytes().contains(&b) {
            out.push_str(&format!("%{b:02X}"));
        } else {
            out.push(b as char);
        }
    }
    out
}

/// An http(s) URL, as much of JS's `URL` as finding icons needs.
///
// shortcut: the host is only lowercased and checked for forbidden characters.
// JS also punycodes non-ASCII hosts, percent-decodes them, and rewrites
// numeric hosts ("127.1" -> "127.0.0.1", "[0:0::1]" -> "[::1]"). Upgrade if
// sites with such addresses turn out to get no icon.
// shortcut: no user:password@ part - such a URL is refused, so that icon
// link is skipped. Upgrade if a site in the wild needs one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Url {
    scheme: String,
    host: String,
    port: Option<u16>,
    /// Always starts with `/`.
    path: String,
    query: Option<String>,
    fragment: Option<String>,
}

/// What `new URL()` made of a string: http(s), or some other valid URL we
/// have no use for (`data:`, `javascript:`, `ftp:`).
// shortcut: every other scheme counts as valid without looking further; JS
// can still reject e.g. "ftp://" for its empty host. Only decides whether a
// broken <base href> is ignored or switches every relative icon link off.
enum Parsed {
    Http(Url),
    Other,
}

impl Url {
    /// `new URL(text)`, http(s) only.
    pub fn parse(text: &str) -> Option<Url> {
        match parse(text, None)? {
            Parsed::Http(url) => Some(url),
            Parsed::Other => None,
        }
    }

    /// `new URL(reference, this)`, http(s) only.
    pub fn join(&self, reference: &str) -> Option<Url> {
        match parse(reference, Some(&Parsed::Http(self.clone())))? {
            Parsed::Http(url) => Some(url),
            Parsed::Other => None,
        }
    }

    pub fn href(&self) -> String {
        let mut out = format!("{}://{}", self.scheme, self.host);
        if let Some(port) = self.port {
            out.push_str(&format!(":{port}"));
        }
        out.push_str(&self.path);
        if let Some(query) = &self.query {
            out.push('?');
            out.push_str(query);
        }
        if let Some(fragment) = &self.fragment {
            out.push('#');
            out.push_str(fragment);
        }
        out
    }

    pub fn hostname(&self) -> &str {
        &self.host
    }

    pub fn pathname(&self) -> &str {
        &self.path
    }
}

fn split_scheme(text: &str) -> Option<(&str, &str)> {
    let colon = text.find(':')?;
    let scheme = &text[..colon];
    let valid = scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    valid.then(|| (scheme, &text[colon + 1..]))
}

fn parse(input: &str, base: Option<&Parsed>) -> Option<Parsed> {
    let text: String =
        input.trim_matches(|c: char| c <= ' ').chars().filter(|c| !matches!(c, '\t' | '\n' | '\r')).collect();
    if let Some((scheme, rest)) = split_scheme(&text) {
        let scheme = scheme.to_ascii_lowercase();
        if scheme != "http" && scheme != "https" {
            return Some(Parsed::Other);
        }
        return match base {
            // "https:icon.png" on an https page is a relative link.
            Some(Parsed::Http(b)) if b.scheme == scheme => relative(rest, b),
            _ => authority(&scheme, rest.trim_start_matches(['/', '\\'])),
        }
        .map(Parsed::Http);
    }
    match base? {
        Parsed::Http(b) => relative(&text, b).map(Parsed::Http),
        Parsed::Other => Some(Parsed::Other),
    }
}

/// Splits "path?query#fragment", encoding the last two.
fn split_tail(text: &str) -> (String, Option<String>, Option<String>) {
    let (text, fragment) = match text.split_once('#') {
        Some((t, f)) => (t, Some(percent_encode(f, FRAGMENT_SET))),
        None => (text, None),
    };
    let (path, query) = match text.split_once('?') {
        Some((p, q)) => (p, Some(percent_encode(q, QUERY_SET))),
        None => (text, None),
    };
    // Browsers read "\" as "/" in http(s) paths.
    (path.replace('\\', "/"), query, fragment)
}

/// `rest` is what follows "scheme://".
fn authority(scheme: &str, rest: &str) -> Option<Url> {
    let end = rest.find(['/', '\\', '?', '#']).unwrap_or(rest.len());
    let (hostport, tail) = rest.split_at(end);
    let (host, port) = if hostport.starts_with('[') {
        let close = hostport.find(']')?;
        hostport.split_at(close + 1)
    } else {
        hostport.split_at(hostport.find(':').unwrap_or(hostport.len()))
    };

    let host = host.to_ascii_lowercase();
    let valid_host = match host.strip_prefix('[') {
        Some(v6) => v6.len() > 1 && v6[..v6.len() - 1].chars().all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.'),
        None => !host.is_empty() && !host.chars().any(|c| c <= ' ' || c == '\u{7f}' || "#/<>?@[\\]^|%:".contains(c)),
    };
    if !valid_host {
        return None;
    }

    let port = match port {
        "" | ":" => None,
        _ => {
            let digits = port.strip_prefix(':')?;
            if !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            Some(digits.parse::<u16>().ok()?)
        }
    };
    let default_port = if scheme == "http" { 80 } else { 443 };

    let (path, query, fragment) = split_tail(tail);
    Some(Url {
        scheme: scheme.to_string(),
        host,
        port: port.filter(|&p| p != default_port),
        path: normalize_path(Vec::new(), path.strip_prefix('/').unwrap_or(&path)),
        query,
        fragment,
    })
}

fn relative(reference: &str, base: &Url) -> Option<Url> {
    let (path, query, fragment) = split_tail(reference);
    if path.starts_with("//") {
        return authority(&base.scheme, reference.trim_start_matches(['/', '\\']));
    }
    let mut url = base.clone();
    url.fragment = fragment;
    if path.is_empty() {
        if query.is_some() {
            url.query = query;
        }
        return Some(url);
    }
    url.query = query;
    url.path = match path.strip_prefix('/') {
        Some(absolute) => normalize_path(Vec::new(), absolute),
        None => {
            let mut dir: Vec<String> = base.path[1..].split('/').map(String::from).collect();
            dir.pop();
            normalize_path(dir, &path)
        }
    };
    Some(url)
}

/// Appends `path` to the segments in `out`, resolving "." and "..".
fn normalize_path(mut out: Vec<String>, path: &str) -> String {
    let count = path.split('/').count();
    for (i, segment) in path.split('/').enumerate() {
        let last = i + 1 == count;
        match segment.to_ascii_lowercase().replace("%2e", ".").as_str() {
            ".." => {
                out.pop();
                if last {
                    out.push(String::new());
                }
            }
            "." => {
                if last {
                    out.push(String::new());
                }
            }
            _ => out.push(percent_encode(segment, PATH_SET)),
        }
    }
    format!("/{}", out.join("/"))
}

/// Accepts what people type into the URL field ("example.com/page" or a
/// full URL). Only http(s) - other schemes (steam://, mailto:) have no
/// favicon to fetch.
pub fn normalize_url(input: &str) -> Option<Url> {
    let text = input.trim_matches(is_js_space);
    if text.is_empty() {
        return None;
    }
    // "localhost:3000" is a host and port, not a scheme.
    let has_scheme = split_scheme(text).is_some_and(|(_, rest)| !rest.starts_with(|c: char| c.is_ascii_digit()));
    if has_scheme {
        Url::parse(text)
    } else {
        Url::parse(&format!("https://{text}"))
    }
}

// ---- <link> tags ----

fn decode_entities(text: &str) -> String {
    let c: Vec<char> = text.chars().collect();
    let word = |ch: char| ch.is_ascii_alphanumeric() || ch == '_';
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        if c[i] == '&' {
            // &#x41; &#65; or &name; - the run of characters each form allows.
            let (start, radix): (usize, Option<u32>) = match (c.get(i + 1), c.get(i + 2)) {
                (Some('#'), Some('x' | 'X')) if c.get(i + 3).is_some_and(|d| d.is_ascii_hexdigit()) => (i + 3, Some(16)),
                (Some('#'), _) => (i + 2, Some(10)),
                _ => (i + 1, None),
            };
            let len = c[start..].iter().take_while(|&&ch| radix.map_or(word(ch), |r| ch.is_digit(r))).count();
            if len > 0 && c.get(start + len) == Some(&';') {
                let body: String = c[start..start + len].iter().collect();
                let decoded = match radix {
                    // A lone surrogate ends up as U+FFFD in a URL either way.
                    Some(r) => match u32::from_str_radix(&body, r) {
                        Ok(0xD800..=0xDFFF) => Some('\u{fffd}'),
                        Ok(n) => char::from_u32(n),
                        Err(_) => None,
                    },
                    None => match body.to_ascii_lowercase().as_str() {
                        "amp" => Some('&'),
                        "quot" => Some('"'),
                        "apos" => Some('\''),
                        "lt" => Some('<'),
                        "gt" => Some('>'),
                        _ => None,
                    },
                };
                match decoded {
                    Some(ch) => out.push(ch),
                    None => out.extend(&c[i..=start + len]),
                }
                i = start + len + 1;
                continue;
            }
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

/// Attribute names (lowercased) and values of one tag, in order; a repeated
/// attribute's last value is the one that counts (see [`attr`]).
fn parse_attributes(tag: &str) -> Vec<(String, String)> {
    let c: Vec<char> = tag.chars().collect();
    let is_name = |ch: char| !is_js_space(ch) && !matches!(ch, '=' | '/' | '>' | '"' | '\'');
    let mut attrs = Vec::new();
    let mut tag_name_skipped = false;
    let mut i = 0;
    while i < c.len() {
        if !is_name(c[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < c.len() && is_name(c[i]) {
            i += 1;
        }
        let name: String = c[start..i].iter().collect();
        while i < c.len() && is_js_space(c[i]) {
            i += 1;
        }
        let mut value = String::new();
        if c.get(i) == Some(&'=') {
            let mut v = i + 1;
            while v < c.len() && is_js_space(c[v]) {
                v += 1;
            }
            let quoted = match c.get(v) {
                Some(&q @ ('"' | '\'')) => c[v + 1..].iter().position(|&ch| ch == q).map(|n| (v + 1, v + 1 + n)),
                _ => None,
            };
            if let Some((from, to)) = quoted {
                value = c[from..to].iter().collect();
                i = to + 1;
            } else {
                // Unquoted - which an opening quote that never closes is too.
                let from = v;
                while v < c.len() && !is_js_space(c[v]) && c[v] != '>' {
                    v += 1;
                }
                if v > from {
                    value = c[from..v].iter().collect();
                    i = v;
                }
            }
        }
        if !tag_name_skipped {
            tag_name_skipped = true;
            continue;
        }
        attrs.push((name.to_lowercase(), decode_entities(&value)));
    }
    attrs
}

fn attr<'a>(attrs: &'a [(String, String)], name: &str) -> &'a str {
    attrs.iter().rev().find(|(n, _)| n == name).map_or("", |(_, v)| v.as_str())
}

/// Every `<name ...>` tag in `html`, any case.
fn find_tags<'a>(html: &'a str, name: &str) -> Vec<&'a str> {
    let b = html.as_bytes();
    let mut tags = Vec::new();
    let mut i = 0;
    while let Some(open) = b[i..].iter().position(|&x| x == b'<').map(|p| p + i) {
        let after = open + 1 + name.len();
        let is_tag = b.len() > after
            && b[open + 1..after].eq_ignore_ascii_case(name.as_bytes())
            && !(b[after].is_ascii_alphanumeric() || b[after] == b'_');
        if is_tag {
            let Some(close) = b[after..].iter().position(|&x| x == b'>') else { break };
            tags.push(&html[open..=after + close]);
            i = after + close + 1;
        } else {
            i = open + 1;
        }
    }
    tags
}

/// Largest edge from a sizes="16x16 32x32" attribute, or 0 if not given.
fn largest_declared_size(sizes: &str) -> u64 {
    let mut best = 0;
    for token in sizes.split(is_js_space) {
        let Some((w, h)) = token.split_once(['x', 'X']) else { continue };
        let number = |s: &str| (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse().unwrap_or(u64::MAX));
        if let (Some(w), Some(h)) = (number(w), number(h)) {
            best = best.max(w).max(h);
        }
    }
    best
}

/// Icon URLs declared in a page's `<link>` tags, best first. SVG icons are
/// skipped - the app only decodes bitmaps - and so is Safari's monochrome
/// mask-icon (its rel token isn't "icon").
pub fn find_icon_links(html: &str, page_url: &str) -> Vec<String> {
    let Some(mut base) = parse(page_url, None) else { return Vec::new() };
    if let Some(tag) = find_tags(html, "base").first() {
        let href = attr(&parse_attributes(tag), "href").to_string();
        if !href.is_empty() {
            if let Some(url) = parse(&href, Some(&base)) {
                base = url;
            }
        }
    }

    let mut found: Vec<(String, u64)> = Vec::new();
    for tag in find_tags(html, "link") {
        let attrs = parse_attributes(tag);
        let rel = attr(&attrs, "rel").to_lowercase();
        let has_rel = |token: &str| rel.split(is_js_space).any(|r| r == token);
        let touch = has_rel("apple-touch-icon") || has_rel("apple-touch-icon-precomposed");
        if !touch && !has_rel("icon") {
            continue;
        }
        let href = attr(&attrs, "href");
        if href.is_empty() {
            continue;
        }
        let Some(Parsed::Http(url)) = parse(href.trim_matches(is_js_space), Some(&base)) else { continue };
        let path = url.pathname().to_ascii_lowercase();
        if attr(&attrs, "type").to_ascii_lowercase().contains("svg") || path.ends_with(".svg") || path.ends_with(".svgz") {
            continue;
        }
        let href = url.href();
        if found.iter().any(|(seen, _)| *seen == href) {
            continue;
        }

        // Touch icons are almost always 180px PNGs even when sizes is omitted.
        // A plain rel=icon with no sizes is usually a 16/32px favicon.
        let size = match largest_declared_size(attr(&attrs, "sizes")) {
            0 if touch => 180,
            0 => 32,
            n => n,
        };
        found.push((href, size));
    }
    found.sort_by_key(|c| std::cmp::Reverse(c.1));
    found.into_iter().map(|(url, _)| url).collect()
}

// ---- Image sniffing ----

pub fn is_png(buf: &[u8]) -> bool {
    buf.starts_with(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])
}

pub fn is_jpeg(buf: &[u8]) -> bool {
    buf.starts_with(&[0xff, 0xd8, 0xff])
}

fn u16_le(buf: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([buf[at], buf[at + 1]])
}

fn u32_le(buf: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([buf[at], buf[at + 1], buf[at + 2], buf[at + 3]])
}

pub fn is_ico(buf: &[u8]) -> bool {
    buf.len() >= 6 && u16_le(buf, 0) == 0 && u16_le(buf, 2) == 1 && u16_le(buf, 4) > 0
}

/// The formats an icon may arrive in; anything else (an HTML error page
/// served as /favicon.ico) isn't handed to the decoder.
pub fn is_icon_image(buf: &[u8]) -> bool {
    is_png(buf) || is_jpeg(buf) || is_ico(buf)
}

/// One image listed in an .ico file's directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IcoEntry {
    /// Position in the file's directory.
    pub index: usize,
    pub width: u32,
    pub bpp: u16,
    /// Where the image's bytes are in the file: `buf[offset..offset + size]`.
    pub offset: usize,
    pub size: usize,
}

/// The images in an .ico file, largest first (deepest colour first among
/// equals): the order to try decoding them in. Entries pointing outside the
/// file are left out; empty if `buf` isn't an .ico.
pub fn ico_entries(buf: &[u8]) -> Vec<IcoEntry> {
    if !is_ico(buf) {
        return Vec::new();
    }
    let mut entries = Vec::new();
    for index in 0..usize::from(u16_le(buf, 4)) {
        let o = 6 + index * 16;
        if o + 16 > buf.len() {
            break;
        }
        let size = u32_le(buf, o + 8) as usize;
        let offset = u32_le(buf, o + 12) as usize;
        if size == 0 || offset.saturating_add(size) > buf.len() {
            continue;
        }
        let width = if buf[o] == 0 { 256 } else { u32::from(buf[o]) };
        entries.push(IcoEntry { index, width, bpp: u16_le(buf, o + 6), offset, size });
    }
    entries.sort_by(|a, b| b.width.cmp(&a.width).then(b.bpp.cmp(&a.bpp)));
    entries
}

// ---- Where to look for a site's icon ----

/// Icons are scaled down to this edge; also the size asked of Google.
pub const FAVICON_SIZE: u32 = 128;
/// Whole site-direct attempt, then fall back.
pub const SITE_ICON_BUDGET_MS: u64 = 10_000;
pub const REQUEST_TIMEOUT_MS: u64 = 5_000;
/// Icon `<link>`s live in `<head>`.
pub const MAX_HTML_BYTES: usize = 512 * 1024;
pub const MAX_ICON_BYTES: usize = 1024 * 1024;
pub const MAX_LINK_CANDIDATES: usize = 3;

pub struct Response {
    /// Status 200-299.
    pub ok: bool,
    /// The URL after redirects (example.com -> www.example.com); empty if
    /// not known.
    pub url: String,
    /// The Content-Type header, empty if there was none.
    pub content_type: String,
    pub body: Vec<u8>,
}

/// The network and the image decoder, supplied by the app.
pub trait IconSource {
    type Image;

    /// GET `url`, following redirects, reading at most `max_bytes` of the
    /// body and then dropping the connection. `None` if the request failed
    /// or took longer than `timeout_ms`.
    fn get(&mut self, url: &str, max_bytes: usize, timeout_ms: u64) -> Option<Response>;

    /// Decodes PNG, JPEG or ICO `bytes` (for an ICO, the first entry of
    /// [`ico_entries`] that decodes), scaled down to [`FAVICON_SIZE`] if
    /// wider. `None` if it doesn't decode.
    fn decode(&mut self, bytes: &[u8]) -> Option<Self::Image>;

    /// A clock that only moves forward, in milliseconds from any start.
    fn now_ms(&mut self) -> u64;
}

fn fetch_image<S: IconSource>(url: &str, timeout_ms: u64, source: &mut S) -> Option<S::Image> {
    let res = source.get(url, MAX_ICON_BYTES, timeout_ms)?;
    if !res.ok {
        return None;
    }
    let body = &res.body[..res.body.len().min(MAX_ICON_BYTES)];
    if !is_icon_image(body) {
        return None;
    }
    source.decode(body)
}

/// The site's own icon: the best `<link rel="icon">` / apple-touch-icon on
/// the page, then /favicon.ico. Talks only to the site the user added.
fn fetch_site_icon<S: IconSource>(page_url: &Url, source: &mut S) -> Option<S::Image> {
    let start = source.now_ms();
    let mut final_url = page_url.clone();
    let mut candidates = Vec::new();
    // If the page doesn't load, /favicon.ico may still be there.
    if let Some(res) = source.get(&page_url.href(), MAX_HTML_BYTES, REQUEST_TIMEOUT_MS) {
        let landed = if res.url.is_empty() { Some(final_url.clone()) } else { Url::parse(&res.url) };
        if let Some(landed) = landed {
            final_url = landed;
            if res.ok && res.content_type.to_ascii_lowercase().contains("html") {
                let html = String::from_utf8_lossy(&res.body[..res.body.len().min(MAX_HTML_BYTES)]);
                candidates = find_icon_links(&html, &final_url.href());
                candidates.truncate(MAX_LINK_CANDIDATES);
            }
        }
    }

    for fallback in [&final_url, page_url] {
        candidates.extend(fallback.join("/favicon.ico").map(|u| u.href()));
    }
    let mut tried: Vec<&String> = Vec::new();
    for url in &candidates {
        if tried.contains(&url) {
            continue;
        }
        tried.push(url);
        let left = SITE_ICON_BUDGET_MS.saturating_sub(source.now_ms().saturating_sub(start));
        if left == 0 {
            break;
        }
        if let Some(image) = fetch_image(url, left.min(REQUEST_TIMEOUT_MS), source) {
            return Some(image);
        }
    }
    None
}

/// Google's favicon service finds icons for sites that make them hard to
/// get (no /favicon.ico, SVG only, bot-blocking), but it tells Google which
/// sites are in the launcher - hence opt-out via settings.
pub fn google_favicon_url(hostname: &str) -> String {
    format!("https://www.google.com/s2/favicons?sz={FAVICON_SIZE}&domain={}", encode_uri_component(hostname))
}

/// The icon for what the user typed as a site's address: the site's own,
/// then Google's if `google_fallback` (the setting) allows it. `None` for
/// anything that isn't an http(s) address, or when nothing was found.
pub fn fetch_favicon<S: IconSource>(url: &str, google_fallback: bool, source: &mut S) -> Option<S::Image> {
    let page_url = normalize_url(url)?;
    if let Some(image) = fetch_site_icon(&page_url, source) {
        return Some(image);
    }
    if !google_fallback {
        return None;
    }
    fetch_image(&google_favicon_url(page_url.hostname()), REQUEST_TIMEOUT_MS + 1000, source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn href(input: &str) -> Option<String> {
        normalize_url(input).map(|u| u.href())
    }

    #[test]
    fn normalize_url_accepts_bare_hosts_and_http_s_urls_only() {
        assert_eq!(href("example.com/page").as_deref(), Some("https://example.com/page"));
        assert_eq!(href(" http://example.org ").as_deref(), Some("http://example.org/"));
        assert_eq!(href("localhost:3000").as_deref(), Some("https://localhost:3000/"));
        assert_eq!(href("steam://rungameid/440"), None);
        assert_eq!(href("mailto:someone@example.com"), None);
        assert_eq!(href(""), None);
    }

    #[test]
    fn find_icon_links_ranks_by_size_resolves_urls_and_skips_svg_mask_icons() {
        let html = r##"
    <head>
      <link rel="stylesheet" href="/site.css">
      <link rel="icon" href="/favicon-32.png" sizes="32x32">
      <link rel="mask-icon" href="/mask.svg" color="#000">
      <link rel="icon" type="image/svg+xml" href="/icon.svg">
      <link rel="apple-touch-icon" href="touch.png">
      <LINK REL='shortcut icon' HREF='/fav.ico?v=1&amp;x=2'>
      <link rel="icon" href="/favicon-32.png" sizes="32x32">
      <link rel="icon" href="javascript:alert(1)">
      <link rel="icon" sizes="16x16 192x192" href="//cdn.example.net/big.png">
    </head>"##;
        assert_eq!(
            find_icon_links(html, "https://example.com/dir/page"),
            [
                "https://cdn.example.net/big.png",
                "https://example.com/dir/touch.png",
                "https://example.com/favicon-32.png",
                "https://example.com/fav.ico?v=1&x=2",
            ]
        );
    }

    #[test]
    fn find_icon_links_honours_base_href() {
        let html = r#"<base href="https://static.example.com/assets/"><link rel="icon" href="fav.png">"#;
        assert_eq!(find_icon_links(html, "https://example.com/"), ["https://static.example.com/assets/fav.png"]);
    }

    // --- ICO fixtures ---
    // Not ported: ICO decoding moves to WIC in Phase 2 - "decodeIco reads
    // 32bpp BMP entries top-down with premultiplied alpha", "decodeIco reads
    // paletted entries and applies the AND mask", and the truncated-pixel-data
    // half of "decodeIco rejects non-ICO and truncated data".

    fn ico(entries: &[(u32, u16, &[u8])]) -> Vec<u8> {
        let mut header = vec![0u8; 6 + entries.len() * 16];
        header[2] = 1;
        header[4] = entries.len() as u8;
        let mut offset = header.len() as u32;
        for (i, (width, bpp, data)) in entries.iter().enumerate() {
            let o = 6 + i * 16;
            header[o] = if *width == 256 { 0 } else { *width as u8 };
            header[o + 1] = header[o];
            header[o + 4] = 1;
            header[o + 6..o + 8].copy_from_slice(&bpp.to_le_bytes());
            header[o + 8..o + 12].copy_from_slice(&(data.len() as u32).to_le_bytes());
            header[o + 12..o + 16].copy_from_slice(&offset.to_le_bytes());
            offset += data.len() as u32;
        }
        for (_, _, data) in entries {
            header.extend_from_slice(data);
        }
        header
    }

    #[test]
    fn decode_ico_picks_the_largest_entry_and_passes_embedded_png_through() {
        let png = [&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a][..], b"rest"].concat();
        let small = [0u8; 48];
        let file = ico(&[(1, 32, &small), (256, 32, &png)]);
        let entries = ico_entries(&file);
        assert_eq!(entries.iter().map(|e| (e.index, e.width)).collect::<Vec<_>>(), [(1, 256), (0, 1)]);
        let best = &file[entries[0].offset..entries[0].offset + entries[0].size];
        assert_eq!(best, png);
        assert!(is_png(best));
    }

    #[test]
    fn decode_ico_rejects_non_ico_and_truncated_data() {
        assert!(ico_entries(b"<html>not an icon</html>").is_empty());
        assert!(!is_icon_image(b"<html>not an icon</html>"));
    }

    // --- Beyond the JS tests: the hand-written URL, tag and entity code ---

    #[test]
    fn ico_entries_skip_what_points_outside_the_file_and_break_ties_on_depth() {
        let mut file = ico(&[(16, 8, &[1; 4]), (16, 32, &[2; 4]), (32, 4, &[3; 4])]);
        assert_eq!(ico_entries(&file).iter().map(|e| e.index).collect::<Vec<_>>(), [2, 1, 0]);
        file.truncate(file.len() - 1); // the last image no longer fits
        assert_eq!(ico_entries(&file).iter().map(|e| e.index).collect::<Vec<_>>(), [1, 0]);
        file.truncate(6 + 16 + 8); // directory cut short
        assert!(ico_entries(&file).is_empty());
    }

    #[test]
    fn sniffing_goes_by_the_first_bytes() {
        assert!(is_png(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0]));
        assert!(is_jpeg(&[0xff, 0xd8, 0xff, 0xe0]));
        assert!(is_ico(&[0, 0, 1, 0, 1, 0]));
        assert!(!is_ico(&[0, 0, 1, 0, 0, 0])); // no images
        assert!(!is_ico(&[0, 0, 2, 0, 1, 0])); // a cursor
        assert!(!is_icon_image(&[0x89, 0x50]));
        assert!(!is_icon_image(&[]));
    }

    #[test]
    fn js_space_is_regex_backslash_s() {
        for c in ['\t', '\n', '\u{b}', '\u{c}', '\r', ' ', '\u{a0}', '\u{2028}', '\u{3000}', '\u{feff}'] {
            assert!(is_js_space(c), "{c:?}");
        }
        for c in ['\u{85}', '\u{200b}', 'a', '\0'] {
            assert!(!is_js_space(c), "{c:?}");
        }
    }

    #[test]
    fn encode_uri_component_matches_js() {
        assert_eq!(encode_uri_component("Launchpad-Setup-1.0.4.exe"), "Launchpad-Setup-1.0.4.exe");
        assert_eq!(encode_uri_component("a b&c/d?e=f#g+h"), "a%20b%26c%2Fd%3Fe%3Df%23g%2Bh");
        assert_eq!(encode_uri_component("-_.!~*'()"), "-_.!~*'()");
        assert_eq!(encode_uri_component("bücher.de"), "b%C3%BCcher.de");
    }

    // Expected values in the three tables below are what Node's `new URL`
    // printed for the same input, except where marked.
    const BASE: &str = "https://example.com/dir/page?q=1#f";
    const ERR: &str = "ERR";

    fn resolved(reference: &str, base: &str) -> String {
        let Some(base) = parse(base, None) else { return ERR.to_string() };
        match parse(reference, Some(&base)) {
            Some(Parsed::Http(url)) => url.href(),
            Some(Parsed::Other) => "OTHER".to_string(),
            None => ERR.to_string(),
        }
    }

    #[test]
    fn relative_references_resolve_like_new_url() {
        let cases = [
            ("a^b`c{d}e|f\"g<h>i j", "https://example.com/dir/a%5Eb%60c%7Bd%7De|f%22g%3Ch%3Ei%20j"),
            ("?a^b`c{d}e|f\"g<h>i j'k", "https://example.com/dir/page?a^b`c{d}e|f%22g%3Ch%3Ei%20j%27k"),
            ("#a^b`c{d}e|f\"g<h>i j'k", "https://example.com/dir/page?q=1#a^b%60c{d}e|f%22g%3Ch%3Ei%20j'k"),
            ("https:foo.png", "https://example.com/dir/foo.png"),
            ("https:/foo.png", "https://example.com/foo.png"),
            ("http:foo.png", "http://foo.png/"),
            ("https:\\\\h\\x", "https://h/x"),
            ("///h/x", "https://h/x"),
            ("\\\\h\\x", "https://h/x"),
            ("..", "https://example.com/"),
            ("../..", "https://example.com/"),
            ("../../../x", "https://example.com/x"),
            ("./", "https://example.com/dir/"),
            ("a/./b/../c", "https://example.com/dir/a/c"),
            ("a/%2e%2e/b", "https://example.com/dir/b"),
            ("a/.%2E", "https://example.com/dir/"),
            ("a/%2E/b", "https://example.com/dir/a/b"),
            ("a/b/%2e%2E", "https://example.com/dir/a/"),
            ("", "https://example.com/dir/page?q=1"),
            ("?", "https://example.com/dir/page?"),
            ("#", "https://example.com/dir/page?q=1#"),
            ("x?", "https://example.com/dir/x?"),
            ("x#", "https://example.com/dir/x#"),
            ("?q#f", "https://example.com/dir/page?q#f"),
            ("x#a b`c", "https://example.com/dir/x#a%20b%60c"),
            ("/a b/é.png", "https://example.com/a%20b/%C3%A9.png"),
            ("é.png?é#é", "https://example.com/dir/%C3%A9.png?%C3%A9#%C3%A9"),
            ("  x.png  ", "https://example.com/dir/x.png"),
            ("a\tb\nc.png", "https://example.com/dir/abc.png"),
            ("x/..", "https://example.com/dir/"),
            ("x/.", "https://example.com/dir/x/"),
            ("/..//a", "https://example.com//a"),
            ("x/../../../y/./", "https://example.com/y/"),
            ("a;b=c,d", "https://example.com/dir/a;b=c,d"),
            ("%zz%20", "https://example.com/dir/%zz%20"),
            ("/a%2fb", "https://example.com/a%2fb"),
            ("icon.png?x=a:b", "https://example.com/dir/icon.png?x=a:b"),
            ("1a:b.png", "https://example.com/dir/1a:b.png"),
            ("/x\u{1}y", "https://example.com/x%01y"),
            ("\\x.png", "https://example.com/x.png"),
            ("x\\y.png", "https://example.com/dir/x/y.png"),
            ("?a\\b", "https://example.com/dir/page?a\\b"),
            ("http://other/x.png", "http://other/x.png"),
            ("HTTPS://Other/x.PNG", "https://other/x.PNG"),
            // Other schemes: valid URLs, but not ones to fetch an icon from.
            ("data:image/png;base64,AA", "OTHER"),
            ("ftp://h/x", "OTHER"),
            ("foo:bar", "OTHER"),
            ("a:b.png", "OTHER"),
        ];
        for (reference, expected) in cases {
            assert_eq!(resolved(reference, BASE), expected, "{reference:?}");
        }
    }

    #[test]
    fn hosts_and_ports_parse_like_new_url() {
        let cases = [
            ("//EXAMPLE.net:443/x", "https://example.net/x"),
            ("//h:80/x", "https://h:80/x"),
            ("//h:0080/", "https://h:80/"),
            ("//h:65535/", "https://h:65535/"),
            ("//h:65536/", ERR),
            ("//h:99999/", ERR),
            ("//h:8a/", ERR),
            ("//h:+80/", ERR),
            ("//h: 80/", ERR),
            ("//a:b:c/", ERR),
            ("//h:/x", "https://h/x"),
            ("//u:p@h/x", ERR), // JS: https://u:p@h/x (see `Url`)
            ("//exa mple/", ERR),
            ("//h|x/", ERR),
            ("//h^x/", ERR),
            ("//a%b/", ERR),
            ("//h\u{7f}/", ERR),
            ("//", ERR),
            ("//:80/", ERR),
            ("//[::1/", ERR),
            ("//[zz]/", ERR),
            ("//[::1]:8080/x", "https://[::1]:8080/x"),
            ("//[::1]/x", "https://[::1]/x"),
            ("//1.2.3.4/", "https://1.2.3.4/"),
            ("//h./x", "https://h./x"),
            ("//a..b/", "https://a..b/"),
            ("//H_x/", "https://h_x/"),
            ("//h~!$&'()*+,;=/", "https://h~!$&'()*+,;=/"),
            ("//xn--bcher-kva.de/", "https://xn--bcher-kva.de/"),
            ("//h?x", "https://h/?x"),
            ("//h#x", "https://h/#x"),
            ("//h\\x", "https://h/x"),
            ("//h", "https://h/"),
            ("//h/..", "https://h/"),
            ("//h/a/../..", "https://h/"),
            ("//h/%", "https://h/%"),
            ("//h/a?b?c#d#e", "https://h/a?b?c#d#e"),
            // Where this parser knowingly differs from JS (see `Url`):
            ("//bücher.de/x", "https://bücher.de/x"), // JS: https://xn--bcher-kva.de/x
            ("//127.1/x", "https://127.1/x"),         // JS: https://127.0.0.1/x
            ("//0x7f.1/", "https://0x7f.1/"),         // JS: https://127.0.0.1/
            ("//1.2.3.4.5/", "https://1.2.3.4.5/"),   // JS: invalid
            ("//h%41/x", ERR),                        // JS: https://ha/x
            ("//[0:0:0:0:0:0:0:1]/x", "https://[0:0:0:0:0:0:0:1]/x"), // JS: https://[::1]/x
        ];
        for (reference, expected) in cases {
            assert_eq!(resolved(reference, BASE), expected, "{reference:?}");
        }
    }

    #[test]
    fn absolute_urls_parse_like_new_url() {
        let cases = [
            ("https://", None),
            ("https:", None),
            ("https:/", None),
            ("http://?x", None),
            ("http://#x", None),
            ("x.png", None),
            ("/x", None),
            ("http://a b/", None),
            ("http://a<b/", None),
            ("ftp://h/x", None),
            ("https://h", Some("https://h/")),
            ("HTTP://H/", Some("http://h/")),
            ("http:/h/x", Some("http://h/x")),
            ("http:h/x", Some("http://h/x")),
            ("https:////h//x", Some("https://h//x")),
            ("http://h:80", Some("http://h/")),
            ("https://h:443", Some("https://h/")),
            ("http://h:443/", Some("http://h:443/")),
            ("https://h:443/a/../../b", Some("https://h/b")),
            ("https://h/?", Some("https://h/?")),
            ("https://h/#", Some("https://h/#")),
            ("http://u@h", None), // JS: http://u@h/ (see `Url`)
            ("http://h/\u{1}", Some("http://h/")),
            (" \u{0}http://h/ \u{1f}", Some("http://h/")),
            ("ht\ntp://h/", Some("http://h/")),
            ("http://h/a\\b", Some("http://h/a/b")),
            ("http://h\\a", Some("http://h/a")),
        ];
        for (input, expected) in cases {
            assert_eq!(Url::parse(input).map(|u| u.href()).as_deref(), expected, "{input:?}");
        }
        let url = Url::parse("https://www.Example.com:8443/a/b.svg?x#y").unwrap();
        assert_eq!((url.hostname(), url.pathname()), ("www.example.com", "/a/b.svg"));
        assert_eq!(url.join("/favicon.ico").unwrap().href(), "https://www.example.com:8443/favicon.ico");
    }

    #[test]
    fn a_base_that_is_not_http_only_lets_absolute_links_through() {
        assert_eq!(resolved("x.png", "ftp://h/d/"), "OTHER"); // JS: ftp://h/d/x.png
        assert_eq!(resolved("http://a/x.png", "ftp://h/d/"), "http://a/x.png");
        assert_eq!(resolved("http:x.png", "ftp://h/d/"), "http://x.png/");
        assert_eq!(resolved("http://a/x", "data:x"), "http://a/x");
        assert_eq!(resolved("http://a/x", "nonsense"), ERR);
        assert_eq!(find_icon_links(r#"<link rel=icon href="http://a/x.png">"#, "nonsense"), [] as [&str; 0]);
    }

    #[test]
    fn normalize_url_tells_schemes_from_ports() {
        assert_eq!(href("example.com:8080/x").as_deref(), Some("https://example.com:8080/x"));
        assert_eq!(href("HTTP://Example.COM").as_deref(), Some("http://example.com/"));
        assert_eq!(href("https:example.com").as_deref(), Some("https://example.com/"));
        assert_eq!(href("//example.com/x").as_deref(), Some("https://example.com/x"));
        assert_eq!(href("\u{a0}example.com\u{feff}").as_deref(), Some("https://example.com/"));
        assert_eq!(href("example.com/a b?c d").as_deref(), Some("https://example.com/a%20b?c%20d"));
        assert_eq!(href("localhost:"), None); // read as a scheme, like JS
        assert_eq!(href("user:pass@example.com"), None); // "user:" is a scheme too
        assert_eq!(href("file:///C:/x.html"), None);
        assert_eq!(href("C:\\Games\\game.exe"), None);
        assert_eq!(href("about:blank"), None);
        assert_eq!(href("   "), None);
        assert_eq!(href("exa mple.com"), None);
        assert_eq!(href("http://"), None);
        // Non-ASCII hosts are left alone (JS: https://xn--bcher-kva.de/).
        assert_eq!(href("bücher.de").as_deref(), Some("https://bücher.de/"));
    }

    fn attrs(tag: &str) -> Vec<(String, String)> {
        parse_attributes(tag)
    }

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect()
    }

    #[test]
    fn attributes_in_every_quoting_style() {
        assert_eq!(
            attrs(r#"<link rel="icon" href='/a.png' sizes=32x32 crossorigin>"#),
            pairs(&[("rel", "icon"), ("href", "/a.png"), ("sizes", "32x32"), ("crossorigin", "")])
        );
        assert_eq!(attrs("<LINK REL = 'Icon'\n\tHREF\n=\n\"/A.png\">"), pairs(&[("rel", "Icon"), ("href", "/A.png")]));
        // The same oddities as the JS regex:
        assert_eq!(attrs("<link rel=icon href=/a.png/>"), pairs(&[("rel", "icon"), ("href", "/a.png/")]));
        assert_eq!(attrs("<link href=\"a b>"), pairs(&[("href", "\"a"), ("b", "")]));
        assert_eq!(attrs("<link href=>"), pairs(&[("href", "")]));
        assert_eq!(attrs("<link href='a>b' x=\"'\">"), pairs(&[("href", "a>b"), ("x", "'")]));
        assert_eq!(attrs("<link =x \"y\" a=1 a=2>"), pairs(&[("y", ""), ("a", "1"), ("a", "2")]));
        assert_eq!(attr(&attrs("<link a=1 a=2>"), "a"), "2");
        assert_eq!(attr(&attrs("<link a=1>"), "b"), "");
        assert_eq!(attrs("<link title=\"&lt;b&gt; &amp; &quot;c&quot;\">"), pairs(&[("title", "<b> & \"c\"")]));
    }

    #[test]
    fn entities_decode_like_the_js() {
        assert_eq!(decode_entities("a&amp;b&AMP;c&lt;&gt;&quot;&apos;"), "a&b&c<>\"'");
        assert_eq!(decode_entities("&#65;&#x41;&#X6a;&#x1F600;"), "AAj\u{1F600}");
        assert_eq!(decode_entities("&nbsp; &unknown; &amp &; &#; &#x; &#xZZ; &#12a; & amp;"), "&nbsp; &unknown; &amp &; &#; &#x; &#xZZ; &#12a; & amp;");
        assert_eq!(decode_entities("&&amp;&amp;amp;"), "&&&amp;");
        assert_eq!(decode_entities("&#xD800;"), "\u{fffd}");
        // JS throws a RangeError here, losing every icon on the page.
        assert_eq!(decode_entities("&#x110000;&#99999999999999999999;"), "&#x110000;&#99999999999999999999;");
    }

    #[test]
    fn tags_are_found_whatever_their_case() {
        assert_eq!(find_tags("<link a><LINK b><LiNk\nc>", "link"), ["<link a>", "<LINK b>", "<LiNk\nc>"]);
        assert_eq!(find_tags("<link><link/><link-x><linkx><links a><link_ a>", "link"), ["<link>", "<link/>", "<link-x>"]);
        assert_eq!(find_tags("<<link a>é<link é>", "link"), ["<link a>", "<link é>"]);
        assert_eq!(find_tags("<link a", "link"), [] as [&str; 0]);
        assert_eq!(find_tags("<link", "link"), [] as [&str; 0]);
        assert_eq!(find_tags("<base href=x><basefont><BASE y>", "base"), ["<base href=x>", "<BASE y>"]);
    }

    #[test]
    fn sizes_take_the_largest_edge() {
        assert_eq!(largest_declared_size("16x16 32x32"), 32);
        assert_eq!(largest_declared_size("  48X64\t\n16x16 "), 64);
        assert_eq!(largest_declared_size("any"), 0);
        assert_eq!(largest_declared_size(""), 0);
        assert_eq!(largest_declared_size("32 x32 32x 1x2x3 -4x4 3.5x3 ٣x٣"), 0);
        assert_eq!(largest_declared_size("any 96x96"), 96);
        assert_eq!(largest_declared_size("99999999999999999999999x1"), u64::MAX);
    }

    const PAGE: &str = "https://example.com/dir/page";

    #[test]
    fn link_rel_tokens_and_sizes_decide_the_order() {
        let html = r#"
            <link rel="icon" sizes="any" href="any.png">
            <link rel="ICON" href="plain.png">
            <link rel="apple-touch-icon-precomposed" href="pre.png">
            <link href="multi.png" sizes="16x16 64x64 512x512" rel="alternate icon">
            <link rel="apple-touch-icon" sizes="57x57" href="small-touch.png">
            <link rel="icons" href="no1.png">
            <link rel="fluid-icon" href="no2.png">
            <link rel="icon">
            <link rel="icon" href="">
            <link rel=icon href=bare.png sizes=48x48>
        "#;
        assert_eq!(
            find_icon_links(html, PAGE),
            [
                "https://example.com/dir/multi.png",
                "https://example.com/dir/pre.png",
                "https://example.com/dir/small-touch.png",
                "https://example.com/dir/bare.png",
                "https://example.com/dir/any.png",
                "https://example.com/dir/plain.png",
            ]
        );
    }

    #[test]
    fn link_hrefs_resolve_against_the_page() {
        let html = r#"
            <link rel="icon" sizes="6x6" href="//cdn.example.net/p.png">
            <link rel="icon" sizes="5x5" href="/root.png?v=2&amp;w=3">
            <link rel="icon" sizes="4x4" href="../up.png">
            <link rel="icon" sizes="3x3" href="  http://other.example/abs.png  ">
            <link rel="icon" sizes="2x2" href="?only=query">
            <link rel="icon" sizes="1x1" href="//bücher.de/ü.png">
            <link rel="icon" href="data:image/png;base64,AAAA">
            <link rel="icon" href="//bad host/x.png">
        "#;
        assert_eq!(
            find_icon_links(html, PAGE),
            [
                "https://cdn.example.net/p.png",
                "https://example.com/root.png?v=2&w=3",
                "https://example.com/up.png",
                "http://other.example/abs.png",
                "https://example.com/dir/page?only=query",
                "https://bücher.de/%C3%BC.png",
            ]
        );
    }

    #[test]
    fn svg_icons_are_skipped_by_type_or_extension() {
        let html = r#"
            <link rel="icon" href="a.SVG">
            <link rel="icon" href="b.svgz?v=1">
            <link rel="icon" type="IMAGE/SVG+XML" href="c">
            <link rel="icon" href="d.svg.png">
            <link rel="icon" href="e.png?x=.svg">
        "#;
        assert_eq!(find_icon_links(html, PAGE), ["https://example.com/dir/d.svg.png", "https://example.com/dir/e.png?x=.svg"]);
    }

    #[test]
    fn base_tag_details() {
        // Relative to the page; only the first <base> counts.
        let html = r#"<BASE HREF="/assets/"><base href="/other/"><link rel="icon" href="fav.png">"#;
        assert_eq!(find_icon_links(html, PAGE), ["https://example.com/assets/fav.png"]);
        // Unusable or empty: ignored.
        let html = r#"<base href="http://"><link rel="icon" href="fav.png">"#;
        assert_eq!(find_icon_links(html, PAGE), ["https://example.com/dir/fav.png"]);
        let html = r#"<base target="_blank"><link rel="icon" href="fav.png">"#;
        assert_eq!(find_icon_links(html, PAGE), ["https://example.com/dir/fav.png"]);
        // Not http(s): relative links lead nowhere fetchable.
        let html = r#"<base href="ftp://h/d/"><link rel="icon" href="fav.png"><link rel="icon" href="https://a.example/abs.png">"#;
        assert_eq!(find_icon_links(html, PAGE), ["https://a.example/abs.png"]);
    }

    // --- The fallback chain (lib/icons.js fetchFavicon; no JS tests) ---

    const PNG: &[u8] = &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 1];
    const ICO: &[u8] = &[0, 0, 1, 0, 1, 0, 2];

    #[derive(Default)]
    struct Fake {
        pages: HashMap<String, Response>,
        /// (url, max_bytes, timeout_ms) of every request.
        requests: Vec<(String, usize, u64)>,
        now: u64,
        /// How long each request takes.
        request_ms: u64,
        undecodable: Vec<Vec<u8>>,
    }

    impl Fake {
        fn page(mut self, url: &str, ok: bool, landed: &str, content_type: &str, body: &[u8]) -> Self {
            let res = Response { ok, url: landed.into(), content_type: content_type.into(), body: body.to_vec() };
            self.pages.insert(url.to_string(), res);
            self
        }
        fn image(self, url: &str, body: &[u8]) -> Self {
            self.page(url, true, url, "image/x-icon", body)
        }
        fn urls(&self) -> Vec<&str> {
            self.requests.iter().map(|r| r.0.as_str()).collect()
        }
    }

    impl IconSource for Fake {
        type Image = Vec<u8>;

        fn get(&mut self, url: &str, max_bytes: usize, timeout_ms: u64) -> Option<Response> {
            self.requests.push((url.to_string(), max_bytes, timeout_ms));
            self.now += self.request_ms;
            let res = self.pages.get(url)?;
            Some(Response { ok: res.ok, url: res.url.clone(), content_type: res.content_type.clone(), body: res.body.clone() })
        }

        fn decode(&mut self, bytes: &[u8]) -> Option<Self::Image> {
            (!self.undecodable.iter().any(|b| b == bytes)).then(|| bytes.to_vec())
        }

        fn now_ms(&mut self) -> u64 {
            self.now
        }
    }

    const GOOGLE: &str = "https://www.google.com/s2/favicons?sz=128&domain=example.com";

    #[test]
    fn page_links_come_first_best_three_only() {
        let html = br#"<link rel="icon" href="/a.png" sizes="16x16"><link rel="icon" href="/b.png" sizes="64x64">
            <link rel="icon" href="/c.png" sizes="48x48"><link rel="icon" href="/d.png" sizes="32x32">"#;
        let mut fake = Fake::default()
            .page("https://example.com/", true, "https://example.com/", "text/HTML; charset=utf-8", html)
            .image("https://example.com/a.png", PNG)
            .image("https://example.com/favicon.ico", ICO);
        // b, c and d are tried (16px a.png is the fourth), then /favicon.ico.
        assert_eq!(fetch_favicon("example.com", true, &mut fake), Some(ICO.to_vec()));
        assert_eq!(
            fake.urls(),
            [
                "https://example.com/",
                "https://example.com/b.png",
                "https://example.com/c.png",
                "https://example.com/d.png",
                "https://example.com/favicon.ico",
            ]
        );
        assert_eq!(fake.requests[0], ("https://example.com/".to_string(), MAX_HTML_BYTES, REQUEST_TIMEOUT_MS));
        assert_eq!(fake.requests[1].1, MAX_ICON_BYTES);

        let mut fake = Fake::default()
            .page("https://example.com/", true, "", "text/html", html)
            .image("https://example.com/c.png", PNG);
        assert_eq!(fetch_favicon("example.com", true, &mut fake), Some(PNG.to_vec()));
        assert_eq!(fake.urls(), ["https://example.com/", "https://example.com/b.png", "https://example.com/c.png"]);
    }

    #[test]
    fn redirects_move_the_links_and_add_a_second_favicon_ico() {
        let mut fake = Fake::default()
            .page("https://example.com/start", true, "https://www.example.com/home/", "text/html", b"<link rel=icon href=i.png>")
            .image("https://example.com/favicon.ico", PNG);
        assert!(fetch_favicon("example.com/start", false, &mut fake).is_some());
        assert_eq!(
            fake.urls(),
            [
                "https://example.com/start",
                "https://www.example.com/home/i.png",
                "https://www.example.com/favicon.ico",
                "https://example.com/favicon.ico",
            ]
        );
    }

    #[test]
    fn favicon_ico_is_tried_when_the_page_gives_no_links() {
        // Page didn't load at all.
        let mut fake = Fake::default().image("https://example.com/favicon.ico", ICO);
        assert!(fetch_favicon("https://example.com/app?x=1", true, &mut fake).is_some());
        assert_eq!(fake.urls(), ["https://example.com/app?x=1", "https://example.com/favicon.ico"]);

        // An error page's links aren't read, nor is anything that isn't HTML.
        for (ok, content_type) in [(false, "text/html"), (true, "application/json"), (true, "")] {
            let mut fake = Fake::default().page("http://example.com/", ok, "http://example.com/", content_type, b"<link rel=icon href=/i.png>");
            assert_eq!(fetch_favicon("http://example.com", false, &mut fake), None);
            assert_eq!(fake.urls(), ["http://example.com/", "http://example.com/favicon.ico"]);
        }
    }

    #[test]
    fn google_is_asked_last_and_only_if_allowed() {
        let mut fake = Fake::default().image(GOOGLE, PNG);
        assert_eq!(fetch_favicon("example.com", false, &mut fake), None);
        assert_eq!(fake.urls(), ["https://example.com/", "https://example.com/favicon.ico"]);

        let mut fake = Fake::default().image(GOOGLE, PNG);
        assert_eq!(fetch_favicon("example.com", true, &mut fake), Some(PNG.to_vec()));
        assert_eq!(fake.urls(), ["https://example.com/", "https://example.com/favicon.ico", GOOGLE]);
        assert_eq!(fake.requests[2], (GOOGLE.to_string(), MAX_ICON_BYTES, REQUEST_TIMEOUT_MS + 1000));

        assert_eq!(google_favicon_url("localhost"), "https://www.google.com/s2/favicons?sz=128&domain=localhost");
        assert_eq!(google_favicon_url("[::1]"), "https://www.google.com/s2/favicons?sz=128&domain=%5B%3A%3A1%5D");
    }

    #[test]
    fn targets_that_are_not_web_addresses_fetch_nothing() {
        for target in ["steam://rungameid/440", "mailto:someone@example.com", "C:\\Games\\game.exe", "", "file:///C:/page.html"] {
            let mut fake = Fake::default();
            assert_eq!(fetch_favicon(target, true, &mut fake), None);
            assert!(fake.requests.is_empty(), "{target}");
        }
    }

    #[test]
    fn responses_that_are_not_usable_images_are_passed_over() {
        let html = br#"<link rel="icon" href="/a.png" sizes="64x64"><link rel="icon" href="/b.png" sizes="48x48">
            <link rel="icon" href="/c.png" sizes="32x32">"#;
        let broken: &[u8] = &[0xff, 0xd8, 0xff, 0];
        let mut fake = Fake::default()
            .page("https://example.com/", true, "https://example.com/", "text/html", html)
            .page("https://example.com/a.png", false, "", "image/png", PNG) // 404 with a body
            .image("https://example.com/b.png", b"<html>Not found</html>") // never reaches the decoder
            .image("https://example.com/c.png", broken)
            .image("https://example.com/favicon.ico", ICO);
        fake.undecodable.push(broken.to_vec());
        assert_eq!(fetch_favicon("example.com", false, &mut fake), Some(ICO.to_vec()));
        assert_eq!(fake.requests.len(), 5);
    }

    #[test]
    fn oversized_bodies_are_cut_at_the_caps() {
        // A link past the HTML cap isn't seen.
        let mut html = vec![b' '; MAX_HTML_BYTES];
        html.extend_from_slice(b"<link rel=icon href=/late.png>");
        let mut big = PNG.to_vec();
        big.resize(MAX_ICON_BYTES + 10, 7);
        let mut fake = Fake::default()
            .page("https://example.com/", true, "", "text/html", &html)
            .image("https://example.com/favicon.ico", &big);
        let bytes = fetch_favicon("example.com", false, &mut fake).unwrap();
        assert_eq!(bytes.len(), MAX_ICON_BYTES);
        assert_eq!(fake.urls(), ["https://example.com/", "https://example.com/favicon.ico"]);
    }

    #[test]
    fn the_site_gets_ten_seconds_in_all() {
        let html = br#"<link rel="icon" href="/a.png" sizes="64x64"><link rel="icon" href="/b.png" sizes="48x48">
            <link rel="icon" href="/c.png" sizes="32x32">"#;
        let mut fake = Fake::default().page("https://example.com/", true, "", "text/html", html).image(GOOGLE, PNG);
        fake.request_ms = 4000;
        // Page at 0s, a.png at 4s, b.png at 8s with the 2s that are left;
        // at 12s the budget is spent, so no c.png or /favicon.ico.
        assert!(fetch_favicon("example.com", true, &mut fake).is_some());
        assert_eq!(fake.urls(), ["https://example.com/", "https://example.com/a.png", "https://example.com/b.png", GOOGLE]);
        assert_eq!(fake.requests.iter().map(|r| r.2).collect::<Vec<_>>(), [5000, 5000, 2000, 6000]);
    }

    #[test]
    fn a_landing_address_that_is_not_http_leaves_only_favicon_ico() {
        let mut fake = Fake::default().page("https://example.com/", true, "about:blank", "text/html", b"<link rel=icon href=/i.png>");
        assert_eq!(fetch_favicon("example.com", false, &mut fake), None);
        assert_eq!(fake.urls(), ["https://example.com/", "https://example.com/favicon.ico"]);
    }
}
