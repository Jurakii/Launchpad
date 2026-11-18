//! HTTP GET over WinHTTP, so TLS stays in Windows (RUST_PORT_PLAN.md Â§4.2).
//! One session for the process; each request gets its own connection and
//! request handles, closed when it returns. Blocking: call off the UI thread.
use super::{arg, from_wide, wide, Context};
use launchpad_core::{Error, Result};
use serde_json::{json, Value};
use std::ffi::c_void;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Networking::WinHttp::*;

pub struct Response {
    pub status: u16,
    /// The URL after redirects.
    pub url: String,
    /// The Content-Type header, empty if there was none.
    pub content_type: String,
    pub content_length: Option<u64>,
    /// At most `max_bytes` of the body.
    pub body: Vec<u8>,
}

/// A WinHTTP handle, closed on drop.
struct Handle(*mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = WinHttpCloseHandle(self.0);
        }
    }
}

fn handle(h: *mut c_void, what: &str) -> Result<Handle> {
    if h.is_null() {
        Err(windows::core::Error::from_thread()).ctx(what)
    } else {
        Ok(Handle(h))
    }
}

fn session() -> Result<*mut c_void> {
    // Stored as usize: a synchronous WinHTTP session may be used from any thread.
    static SESSION: OnceLock<usize> = OnceLock::new();
    if let Some(&s) = SESSION.get() {
        return Ok(s as *mut c_void);
    }
    let agent = wide(format!("Launchpad/{}", env!("CARGO_PKG_VERSION")));
    let h = unsafe { WinHttpOpen(PCWSTR(agent.as_ptr()), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0) };
    let h = handle(h, "Couldn't start WinHTTP")?;
    // Follow https -> http redirects too, as Chromium's fetch did.
    let policy = WINHTTP_OPTION_REDIRECT_POLICY_ALWAYS.to_ne_bytes();
    unsafe { WinHttpSetOption(Some(h.0), WINHTTP_OPTION_REDIRECT_POLICY, Some(&policy)) }.ctx("WinHTTP redirect policy")?;
    let s = *SESSION.get_or_init(|| {
        let raw = h.0 as usize;
        std::mem::forget(h); // the session lives for the process
        raw
    });
    Ok(s as *mut c_void)
}

fn ms(d: Duration) -> i32 {
    d.as_millis().clamp(1, i32::MAX as u128) as i32
}

/// A header as text, `None` if the response doesn't have it.
fn header(req: &Handle, which: u32) -> Option<String> {
    let mut buf = [0u16; 1024];
    let mut len = std::mem::size_of_val(&buf) as u32;
    unsafe { WinHttpQueryHeaders(req.0, which, PCWSTR::null(), Some(buf.as_mut_ptr().cast()), &mut len, std::ptr::null_mut()) }
        .ok()?;
    Some(from_wide(&buf[..len as usize / 2]))
}

/// GET `url`, following redirects; reads at most `max_bytes` of the body and
/// then drops the connection. The whole request, body included, is given up
/// after `timeout_ms`.
pub fn get(url: &str, max_bytes: usize, timeout_ms: u64) -> Result<Response> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let session = session()?;

    let url_w: Vec<u16> = url.encode_utf16().collect();
    let mut host = [0u16; 256];
    let mut path = vec![0u16; url_w.len() + 1];
    let mut parts = URL_COMPONENTS {
        dwStructSize: std::mem::size_of::<URL_COMPONENTS>() as u32,
        lpszHostName: PWSTR(host.as_mut_ptr()),
        dwHostNameLength: host.len() as u32,
        lpszUrlPath: PWSTR(path.as_mut_ptr()),
        dwUrlPathLength: path.len() as u32,
        ..Default::default()
    };
    // With no ExtraInfo buffer, the query stays on the end of lpszUrlPath.
    unsafe { WinHttpCrackUrl(&url_w, 0, &mut parts) }.map_err(|_| Error::msg(format!("Not a web address: {url}")))?;
    let secure = match parts.nScheme {
        WINHTTP_INTERNET_SCHEME_HTTPS => WINHTTP_FLAG_SECURE,
        WINHTTP_INTERNET_SCHEME_HTTP => WINHTTP_OPEN_REQUEST_FLAGS(0),
        _ => return Err(Error::msg(format!("Not an http(s) address: {url}"))),
    };

    let conn = handle(unsafe { WinHttpConnect(session, PCWSTR(host.as_ptr()), parts.nPort, 0) }, "Couldn't connect")?;
    let req = handle(
        unsafe { WinHttpOpenRequest(conn.0, w!("GET"), PCWSTR(path.as_ptr()), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), secure) },
        "Couldn't open the request",
    )?;
    let t = ms(deadline.saturating_duration_since(Instant::now()));
    unsafe { WinHttpSetTimeouts(req.0, t, t, t, t) }.ctx("Couldn't set timeouts")?;

    // WinHTTP's own receive timeout fires late once a body has started (4 s
    // for 1 s here), so a watchdog closes the request at the deadline: the
    // documented way to cancel a synchronous call. A call made after that
    // gets ERROR_INVALID_HANDLE from WinHTTP's handle table.
    let (done, rx) = std::sync::mpsc::channel::<()>();
    let raw = req.0 as usize;
    let left = deadline.saturating_duration_since(Instant::now());
    let watchdog = std::thread::spawn(move || {
        let fired = rx.recv_timeout(left) == Err(std::sync::mpsc::RecvTimeoutError::Timeout);
        if fired {
            unsafe {
                let _ = WinHttpCloseHandle(raw as *mut c_void);
            }
        }
        fired
    });
    let result = read(&req, url, max_bytes);
    drop(done);
    if watchdog.join().unwrap_or(false) {
        std::mem::forget(req); // already closed by the watchdog
        return Err(Error::msg(format!("Timed out: {url}")));
    }
    result
}

fn fail<'a>(what: &'a str, url: &'a str) -> impl Fn(windows::core::Error) -> Error + 'a {
    move |e| {
        // WinHTTP's codes (12xxx) have no system message text.
        let code = e.code().0 as u32 & 0xFFFF;
        let why = match code {
            12002 => "timed out".to_string(),
            12007 => "the server name couldn't be resolved".to_string(),
            12029 => "couldn't connect".to_string(),
            12175 => "the secure connection failed".to_string(),
            _ if e.message().is_empty() => format!("WinHTTP error {code}"),
            _ => e.message(),
        };
        Error::msg(format!("{what} ({why}): {url}"))
    }
}

fn read(req: &Handle, url: &str, max_bytes: usize) -> Result<Response> {
    unsafe { WinHttpSendRequest(req.0, None, None, 0, 0, 0) }.map_err(fail("Request failed", url))?;
    unsafe { WinHttpReceiveResponse(req.0, std::ptr::null_mut()) }.map_err(fail("No response", url))?;

    let status = header(req, WINHTTP_QUERY_STATUS_CODE).and_then(|s| s.trim().parse().ok()).unwrap_or(0);
    let content_type = header(req, WINHTTP_QUERY_CONTENT_TYPE).unwrap_or_default();
    let content_length = header(req, WINHTTP_QUERY_CONTENT_LENGTH).and_then(|s| s.trim().parse().ok());
    let final_url = {
        let mut buf = vec![0u16; 8192];
        let mut len = (buf.len() * 2) as u32;
        match unsafe { WinHttpQueryOption(req.0, WINHTTP_OPTION_URL, Some(buf.as_mut_ptr().cast()), &mut len) } {
            Ok(()) => from_wide(&buf),
            Err(_) => url.to_string(),
        }
    };

    let mut body = Vec::with_capacity(content_length.map_or(16 * 1024, |n: u64| n.min(max_bytes as u64) as usize));
    let mut chunk = vec![0u8; 64 * 1024];
    while body.len() < max_bytes {
        // Synchronous ReadData waits to fill its buffer; ask only for what has arrived.
        let mut ready = 0u32;
        unsafe { WinHttpQueryDataAvailable(req.0, &mut ready) }.map_err(fail("Reading the response failed", url))?;
        if ready == 0 {
            break;
        }
        let want = chunk.len().min(max_bytes - body.len()).min(ready as usize) as u32;
        let mut got = 0u32;
        unsafe { WinHttpReadData(req.0, chunk.as_mut_ptr().cast(), want, &mut got) }
            .map_err(fail("Reading the response failed", url))?;
        if got == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..got as usize]);
    }
    if content_length.is_some_and(|n| (body.len() as u64) < n.min(max_bytes as u64)) {
        return Err(Error::msg(format!("The response ended early: {url}")));
    }
    Ok(Response { status, url: final_url, content_type, content_length, body })
}

/// `get --url U [--max N] [--timeout MS]`.
pub fn probe(args: &[String]) -> Result<Value> {
    if args.first().map(String::as_str) != Some("get") {
        return Err(Error::msg("Usage: --probe http get --url <url> [--max bytes] [--timeout ms]"));
    }
    let url = arg(args, "--url").ok_or_else(|| Error::msg("--url is required"))?;
    let max = arg(args, "--max").and_then(|s| s.parse().ok()).unwrap_or(64 * 1024);
    let timeout = arg(args, "--timeout").and_then(|s| s.parse().ok()).unwrap_or(10_000);
    let start = Instant::now();
    let res = get(url, max, timeout)?;
    let text = String::from_utf8_lossy(&res.body[..res.body.len().min(300)]).into_owned();
    Ok(json!({
        "status": res.status,
        "url": res.url,
        "contentType": res.content_type,
        "contentLength": res.content_length,
        "bytesRead": res.body.len(),
        "ms": start.elapsed().as_millis() as u64,
        "start": text,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// A local server answering each connection in turn with `replies(port, n)`.
    fn serve(count: usize, reply: impl Fn(u16, usize) -> Vec<u8> + Send + 'static) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for n in 0..count {
                let Ok((mut s, _)) = listener.accept() else { return };
                let mut buf = [0u8; 4096];
                let _ = s.read(&mut buf);
                let _ = s.write_all(&reply(port, n));
                if n + 1 == count {
                    // Hold the last connection open: a stalled body.
                    std::thread::sleep(Duration::from_millis(4000));
                }
            }
        });
        port
    }

    #[test]
    fn follows_redirects_reports_the_final_url_and_caps_the_body() {
        let port = serve(2, |port, n| {
            if n == 0 {
                format!("HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{port}/final\r\nConnection: close\r\nContent-Length: 0\r\n\r\n").into_bytes()
            } else {
                let mut r = b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 100000\r\nConnection: close\r\n\r\n".to_vec();
                r.extend(std::iter::repeat_n(b'x', 100_000));
                r
            }
        });
        let res = get(&format!("http://127.0.0.1:{port}/start"), 1000, 5000).unwrap();
        assert_eq!(res.status, 200);
        assert_eq!(res.url, format!("http://127.0.0.1:{port}/final"));
        assert_eq!(res.content_type, "text/html");
        assert_eq!(res.content_length, Some(100_000));
        assert_eq!(res.body.len(), 1000);
    }

    #[test]
    fn gives_up_after_the_timeout() {
        // Sends headers, promises a body, then stalls.
        let port = serve(1, |_, _| b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nab".to_vec());
        let start = Instant::now();
        let r = get(&format!("http://127.0.0.1:{port}/"), 1000, 600);
        eprintln!("{:?} {:?}", start.elapsed(), r.as_ref().map(|r| r.body.len()).map_err(|e| e.to_string()));
        assert!(r.is_err());
        assert!(start.elapsed() < Duration::from_millis(1500));
    }

    #[test]
    fn refuses_other_schemes() {
        assert!(get("ftp://example.com/", 10, 1000).is_err());
        assert!(get("not a url", 10, 1000).is_err());
    }
}
