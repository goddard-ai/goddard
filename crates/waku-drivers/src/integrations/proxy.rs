//! The local MCP proxy. Agents connect to `http://127.0.0.1:<port>/mcp/<id>`
//! with the per-install bearer token; the proxy injects the real upstream
//! credential and relays the request through `curl`, re-framing the response
//! (SSE included) before it reaches the client — `curl -i` prints the
//! upstream's headers but writes the body already de-chunked.
//!
//! Plain HTTP/1.1, thread per connection, keep-alive supported because
//! streamable-HTTP clients reuse connections.

use std::io::Read as _;
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;

use anyhow::{Context as _, anyhow};

use super::Inner;
use super::http::{self, CurlJob};

const MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_BODY_BYTES: usize = 32 * 1024 * 1024;

/// Bind the loopback listener before `Inner` exists so its address can live
/// on `Inner`; [`run`] then starts accepting.
pub fn bind() -> anyhow::Result<TcpListener> {
    TcpListener::bind("127.0.0.1:0").context("could not bind the integrations MCP proxy")
}

pub fn run(listener: TcpListener, inner: Arc<Inner>) {
    let result = std::thread::Builder::new()
        .name("goddard-mcp-proxy".into())
        .spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => {
                        let inner = inner.clone();
                        let _ = std::thread::Builder::new()
                            .name("goddard-mcp-relay".into())
                            .spawn(move || serve(stream, inner));
                    }
                    Err(error) => {
                        eprintln!("goddard-mcp: accept failed: {error}");
                    }
                }
            }
        });
    if let Err(error) = result {
        eprintln!("goddard-mcp: could not start the proxy thread: {error}");
    }
}

fn serve(mut stream: TcpStream, inner: Arc<Inner>) {
    // Pipelined bytes past one request's end seed the next read.
    let mut pending = Vec::new();
    while let Ok(request) = read_request(&mut stream, &mut pending) {
        let Some(request) = request else { return };
        if handle(&mut stream, request, &inner).is_err() {
            return;
        }
    }
}

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

/// Read one request; `Ok(None)` is a clean close. `pending` carries bytes
/// read ahead of the previous request's end.
fn read_request(stream: &mut TcpStream, pending: &mut Vec<u8>) -> anyhow::Result<Option<Request>> {
    let mut buffer = std::mem::take(pending);
    let header_end = loop {
        if let Some(end) = find_header_end(&buffer) {
            break end;
        }
        if buffer.len() > MAX_HEADER_BYTES {
            write_simple(stream, 431, "header block too large");
            return Ok(None);
        }
        let mut chunk = [0_u8; 8192];
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Ok(None);
        }
        buffer.extend_from_slice(&chunk[..read]);
    };
    let head = String::from_utf8_lossy(&buffer[..header_end]).into_owned();
    let mut body = buffer.split_off(header_end + 4);
    let mut lines = head.lines();
    let mut request_line = lines.next().unwrap_or_default().split_whitespace();
    let (Some(method), Some(path)) = (request_line.next(), request_line.next()) else {
        write_simple(stream, 400, "malformed request");
        return Ok(None);
    };
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.trim().to_owned(), value.trim().to_owned()))
        })
        .collect();
    let header = |name: &str| {
        headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    };

    let chunked = header("transfer-encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked"));
    let request_body = if chunked {
        read_chunked(stream, &mut body)?
    } else {
        let length = header("content-length")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0);
        if length > MAX_BODY_BYTES {
            write_simple(stream, 413, "body too large");
            return Ok(None);
        }
        read_exact_body(stream, &mut body, length)?
    };
    // `body` now holds whatever was read past this request's end.
    *pending = body;
    let body = request_body;
    Ok(Some(Request {
        method: method.to_owned(),
        path: path.to_owned(),
        headers,
        body,
    }))
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|w| w == b"\r\n\r\n")
}

fn read_exact_body(
    stream: &mut TcpStream,
    buffered: &mut Vec<u8>,
    length: usize,
) -> anyhow::Result<Vec<u8>> {
    let mut body = std::mem::take(buffered);
    body.reserve(length.saturating_sub(body.len()));
    while body.len() < length {
        let mut chunk = [0_u8; 8192];
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Err(anyhow!("connection closed mid-body"));
        }
        body.extend_from_slice(&chunk[..read]);
    }
    *buffered = body.split_off(length);
    Ok(body)
}

fn read_chunked(stream: &mut TcpStream, buffered: &mut Vec<u8>) -> anyhow::Result<Vec<u8>> {
    let mut body = Vec::new();
    loop {
        let size_line = read_line(stream, buffered)?;
        let size =
            usize::from_str_radix(size_line.trim().split(';').next().unwrap_or_default(), 16)
                .map_err(|_| anyhow!("bad chunk size"))?;
        if size == 0 {
            // Trailers until an empty line.
            while !read_line(stream, buffered)?.trim().is_empty() {}
            return Ok(body);
        }
        if body.len() + size > MAX_BODY_BYTES {
            return Err(anyhow!("body too large"));
        }
        let chunk = read_exact_body(stream, buffered, size)?;
        body.extend_from_slice(&chunk);
        let crlf = read_exact_body(stream, buffered, 2)?;
        if crlf != b"\r\n" {
            return Err(anyhow!("bad chunk terminator"));
        }
    }
}

fn read_line(stream: &mut TcpStream, buffered: &mut Vec<u8>) -> anyhow::Result<String> {
    loop {
        if let Some(end) = buffered.windows(2).position(|w| w == b"\r\n") {
            let line = String::from_utf8_lossy(&buffered[..end]).into_owned();
            buffered.drain(..end + 2);
            return Ok(line);
        }
        let mut chunk = [0_u8; 8192];
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Err(anyhow!("connection closed mid-chunk"));
        }
        buffered.extend_from_slice(&chunk[..read]);
    }
}

fn handle(stream: &mut TcpStream, request: Request, inner: &Arc<Inner>) -> anyhow::Result<()> {
    let id = request
        .path
        .strip_prefix("/mcp/")
        .map(|rest| rest.trim_end_matches('/').to_owned())
        .filter(|rest| !rest.is_empty() && !rest.contains('/'));
    let Some(id) = id else {
        write_simple(stream, 404, "unknown integration endpoint");
        return Ok(());
    };

    let authorized = request
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
        .and_then(|(_, value)| value.strip_prefix("Bearer "))
        .is_some_and(|token| inner.permits(token, &id));
    if !authorized {
        write_simple(stream, 401, "missing token or integration not granted");
        return Ok(());
    }

    match inner.upstream(&id) {
        Ok(Some(upstream)) => relay(stream, request, upstream, inner),
        Ok(None) => {
            write_simple(stream, 404, "integration is not connected");
            Ok(())
        }
        Err(error) => {
            write_simple(stream, 502, &format!("upstream unavailable: {error:#}"));
            Ok(())
        }
    }
}

/// Hop-by-hop and proxy-owned headers never travel upstream.
const SKIP_HEADERS: &[&str] = &[
    "authorization",
    "connection",
    "content-length",
    "host",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

fn relay(
    stream: &mut TcpStream,
    request: Request,
    upstream: super::Upstream,
    inner: &Arc<Inner>,
) -> anyhow::Result<()> {
    let body_file = if request.body.is_empty() {
        None
    } else {
        let path = http::body_file(
            &inner.data_dir,
            &format!("req-{}", uuid::Uuid::new_v4().simple()),
        )?;
        std::fs::write(&path, &request.body)?;
        Some(path)
    };
    let mut headers: Vec<(&str, String)> = request
        .headers
        .iter()
        .filter(|(name, _)| !SKIP_HEADERS.contains(&name.to_ascii_lowercase().as_str()))
        .map(|(name, value)| (name.as_str(), value.clone()))
        .collect();
    for (name, value) in &upstream.headers {
        headers.push((name.as_str(), value.clone()));
    }
    let job = CurlJob {
        method: &request.method,
        url: &upstream.url,
        headers: &headers,
        body_file: body_file.as_deref(),
        follow: false,
    };
    let mut child = match http::spawn_with(&job, std::process::Stdio::null()) {
        // The spawn permit only throttles spawns; a long-lived SSE response
        // must not hold it for the stream's whole life.
        Ok(child) => child.into_inner(),
        Err(error) => {
            if let Some(path) = &body_file {
                let _ = std::fs::remove_file(path);
            }
            write_simple(stream, 502, &format!("could not reach upstream: {error:#}"));
            return Ok(());
        }
    };
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("curl stdout unavailable"))?;
    let head_only = request.method.eq_ignore_ascii_case("HEAD");
    let persist = relay_response(&mut stdout, stream, head_only, || {
        child.wait().map(|status| status.success()).unwrap_or(false)
    });
    let _ = child.kill();
    let _ = child.wait();
    // curl reads the body file during the request; remove it only once the
    // child is done.
    if let Some(path) = &body_file {
        let _ = std::fs::remove_file(path);
    }
    match persist {
        Ok(Persist::KeepAlive) => Ok(()),
        Ok(Persist::Close) => Err(anyhow!("upstream ended the response early")),
        Err(error) => Err(error),
    }
}

enum Persist {
    KeepAlive,
    Close,
}

/// Response headers the upstream owned — the proxy reframes the response,
/// so hop-by-hop values cannot leak to the client.
const DROP_RESPONSE_HEADERS: &[&str] = &["connection", "keep-alive", "transfer-encoding"];

/// Pipe one `curl -i` response from `upstream` to `client`, fixing the
/// framing: curl prints the upstream's head untouched but writes the body
/// already de-chunked, so the upstream's `Transfer-Encoding` and
/// `Content-Length` cannot be trusted to describe what follows.
///
/// `head_only` is the request's HEAD-ness; `upstream_ok` is called once, at
/// upstream EOF, and reports whether curl exited cleanly — it decides
/// whether a streamed body is complete or truncated.
fn relay_response(
    upstream: &mut impl std::io::Read,
    client: &mut impl std::io::Write,
    head_only: bool,
    upstream_ok: impl FnOnce() -> bool,
) -> anyhow::Result<Persist> {
    // Interim 1xx heads arrive first, each without a body.
    let mut buffered = Vec::new();
    let (code, reason, headers) = loop {
        let head_end = loop {
            if let Some(end) = find_header_end(&buffered) {
                break end;
            }
            if buffered.len() > MAX_HEADER_BYTES {
                write_simple(client, 502, "upstream request failed");
                return Ok(Persist::KeepAlive);
            }
            let mut chunk = [0_u8; 8192];
            let read = upstream.read(&mut chunk)?;
            if read == 0 {
                // curl wrote nothing — it never got a response.
                write_simple(client, 502, "upstream request failed");
                return Ok(Persist::KeepAlive);
            }
            buffered.extend_from_slice(&chunk[..read]);
        };
        let head = String::from_utf8_lossy(&buffered[..head_end]).into_owned();
        buffered.drain(..head_end + 4);
        let mut lines = head.lines();
        let mut status = lines.next().unwrap_or_default().splitn(3, ' ');
        let parsed = status
            .next()
            .filter(|version| version.starts_with("HTTP/"))
            .and_then(|_| status.next())
            .and_then(|code| code.parse::<u16>().ok());
        let Some(code) = parsed else {
            write_simple(client, 502, "upstream request failed");
            return Ok(Persist::KeepAlive);
        };
        if (100..200).contains(&code) {
            continue;
        }
        let reason = status.next().unwrap_or_default().to_owned();
        let headers: Vec<(String, String)> = lines
            .filter_map(|line| {
                let (name, value) = line.split_once(':')?;
                Some((name.trim().to_owned(), value.trim().to_owned()))
            })
            .collect();
        break (code, reason, headers);
    };
    let header = |name: &str| {
        headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    };
    let no_body = head_only || matches!(code, 204 | 304);
    // curl already de-chunked a chunked body; only a close-delimited or
    // Content-Length-less head needs re-framing, and only a real
    // Content-Length head can keep its own framing.
    let keep_length = !no_body
        && header("transfer-encoding").is_none()
        && header("content-length")
            .and_then(|value| value.parse::<usize>().ok())
            .is_some();

    let mut head = if reason.is_empty() {
        format!("HTTP/1.1 {code}\r\n")
    } else {
        format!("HTTP/1.1 {code} {reason}\r\n")
    };
    for (name, value) in &headers {
        let lower = name.to_ascii_lowercase();
        if DROP_RESPONSE_HEADERS.contains(&lower.as_str()) {
            continue;
        }
        if !no_body && !keep_length && lower == "content-length" {
            continue;
        }
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    if !no_body && !keep_length {
        head.push_str("Transfer-Encoding: chunked\r\n");
    }
    head.push_str("\r\n");
    client.write_all(head.as_bytes())?;
    client.flush()?;

    if no_body {
        return Ok(Persist::KeepAlive);
    }
    if keep_length {
        let mut remaining = header("content-length")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or_default();
        while remaining > 0 {
            if buffered.is_empty() {
                let mut chunk = [0_u8; 8192];
                let read = upstream.read(&mut chunk)?;
                if read == 0 {
                    return Ok(Persist::Close);
                }
                buffered.extend_from_slice(&chunk[..read]);
            }
            let take = remaining.min(buffered.len());
            client.write_all(&buffered[..take])?;
            buffered.drain(..take);
            remaining -= take;
        }
        client.flush()?;
        return Ok(Persist::KeepAlive);
    }
    // Re-chunk the de-chunked body; each read flushes so SSE events reach
    // the client as they arrive.
    loop {
        if buffered.is_empty() {
            let mut chunk = [0_u8; 8192];
            let read = upstream.read(&mut chunk)?;
            if read == 0 {
                if upstream_ok() {
                    client.write_all(b"0\r\n\r\n")?;
                    client.flush()?;
                    return Ok(Persist::KeepAlive);
                }
                // No terminator: the client must see a truncated stream,
                // not a falsely complete body.
                return Ok(Persist::Close);
            }
            buffered.extend_from_slice(&chunk[..read]);
        }
        client.write_all(format!("{:x}\r\n", buffered.len()).as_bytes())?;
        client.write_all(&buffered)?;
        client.write_all(b"\r\n")?;
        client.flush()?;
        buffered.clear();
    }
}

fn write_simple(stream: &mut impl std::io::Write, status: u16, message: &str) {
    let reason = match status {
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        413 => "Content Too Large",
        431 => "Request Header Fields Too Large",
        502 => "Bad Gateway",
        _ => "Error",
    };
    let body = serde_json::json!({ "error": message }).to_string();
    let _ = write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.flush();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    fn stream_pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        (client, server)
    }

    #[test]
    fn reads_a_post_with_a_content_length_body() {
        let (mut client, mut server) = stream_pair();
        client
            .write_all(
                b"POST /mcp/linear HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 4\r\n\r\nbody",
            )
            .unwrap();
        let mut pending = Vec::new();
        let request = read_request(&mut server, &mut pending).unwrap().unwrap();
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/mcp/linear");
        assert_eq!(request.body, b"body");
    }

    #[test]
    fn reads_a_chunked_body_and_preserves_pipelined_bytes() {
        let (mut client, mut server) = stream_pair();
        client
            .write_all(
                b"POST /mcp/x HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nbody\r\n0\r\n\r\nGET /mcp/x HTTP/1.1\r\nContent-Length: 0\r\n\r\n",
            )
            .unwrap();
        let mut pending = Vec::new();
        let first = read_request(&mut server, &mut pending).unwrap().unwrap();
        assert_eq!(first.body, b"body");
        let second = read_request(&mut server, &mut pending).unwrap().unwrap();
        assert_eq!(second.method, "GET");
        assert!(second.body.is_empty());
    }

    #[test]
    fn a_clean_close_is_none() {
        let (client, mut server) = stream_pair();
        drop(client);
        let mut pending = Vec::new();
        assert!(read_request(&mut server, &mut pending).unwrap().is_none());
    }

    #[test]
    fn relay_response_skips_interim_and_honors_content_length() {
        let mut upstream =
            std::io::Cursor::new(b"HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello".to_vec());
        let mut client = Vec::new();
        let persist = relay_response(&mut upstream, &mut client, false, || true).unwrap();
        assert!(matches!(persist, Persist::KeepAlive));
        let text = String::from_utf8(client).unwrap();
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert_eq!(text.matches("HTTP/").count(), 1);
        assert!(text.contains("Content-Length: 5\r\n"));
        assert!(!text.to_ascii_lowercase().contains("connection"));
        assert!(text.ends_with("\r\n\r\nhello"));
    }

    #[test]
    fn relay_response_does_not_terminate_a_truncated_stream() {
        let mut upstream = std::io::Cursor::new(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\npartial".to_vec(),
        );
        let mut client = Vec::new();
        let persist = relay_response(&mut upstream, &mut client, false, || false).unwrap();
        assert!(matches!(persist, Persist::Close));
        let text = String::from_utf8(client).unwrap();
        assert!(text.ends_with("7\r\npartial\r\n"));
        assert!(!text.contains("0\r\n\r\n"));
    }

    #[cfg(unix)]
    mod end_to_end {
        use super::*;
        use crate::integrations::service::IntegrationService;
        use crate::settings::DaemonSettingsStore;
        use std::sync::Arc;
        use std::time::Duration;
        use waku_protocol::integrations::{McpServerSetting, McpServerTransport};

        const SSE_EVENT: &str =
            "data: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"tools\":[]}}\n\n";

        fn fixture(url: String) -> (IntegrationService, std::path::PathBuf) {
            let root = std::env::temp_dir().join(format!("mcp-proxy-{}", uuid::Uuid::new_v4()));
            let settings = Arc::new(DaemonSettingsStore::open(root.join("settings.json")).unwrap());
            let mut document = settings.get();
            document.integrations_enabled = true;
            document.mcp_servers.push(McpServerSetting {
                id: "upstream".to_owned(),
                name: String::new(),
                transport: McpServerTransport::Http {
                    url,
                    headers: Default::default(),
                },
                providers: Vec::new(),
            });
            settings.replace(document).unwrap();
            (
                IntegrationService::new(settings, root.clone()).unwrap(),
                root,
            )
        }

        fn connect(service: &IntegrationService) -> TcpStream {
            let url = service.endpoint_url("upstream");
            let address = url
                .strip_prefix("http://")
                .unwrap()
                .split('/')
                .next()
                .unwrap()
                .to_owned();
            let stream = TcpStream::connect(address).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            stream
        }

        fn post(stream: &mut TcpStream, service: &IntegrationService, body: &str) {
            write!(
                stream,
                "POST /mcp/upstream HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                service.proxy_token(),
                body.len()
            )
            .unwrap();
            stream.flush().unwrap();
        }

        fn read_head(stream: &mut TcpStream, buffered: &mut Vec<u8>) -> String {
            loop {
                if let Some(end) = find_header_end(buffered) {
                    let head = String::from_utf8_lossy(&buffered[..end]).into_owned();
                    buffered.drain(..end + 4);
                    return head;
                }
                let mut chunk = [0_u8; 8192];
                let read = stream.read(&mut chunk).unwrap();
                assert!(read > 0, "connection closed before a response head");
                buffered.extend_from_slice(&chunk[..read]);
            }
        }

        /// An upstream answering `count` requests — one connection each,
        /// because every relay spawns a fresh curl.
        fn serve_sse_requests(count: usize) -> (String, std::thread::JoinHandle<()>) {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/mcp", listener.local_addr().unwrap());
            let handle = std::thread::spawn(move || {
                for _ in 0..count {
                    let (mut socket, _) = listener.accept().unwrap();
                    socket
                        .set_read_timeout(Some(Duration::from_secs(10)))
                        .unwrap();
                    let mut pending = Vec::new();
                    read_request(&mut socket, &mut pending).unwrap().unwrap();
                    write!(
                        socket,
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{}\r\n0\r\n\r\n",
                        SSE_EVENT.len(),
                        SSE_EVENT
                    )
                    .unwrap();
                    socket.flush().unwrap();
                }
            });
            (url, handle)
        }

        #[test]
        fn streams_chunked_sse_over_a_kept_alive_connection() {
            let (url, upstream) = serve_sse_requests(2);
            let (service, root) = fixture(url);
            let mut client = connect(&service);
            let mut buffered = Vec::new();
            for _ in 0..2 {
                post(&mut client, &service, "{\"jsonrpc\":\"2.0\",\"id\":1}");
                let head = read_head(&mut client, &mut buffered);
                assert!(head.starts_with("HTTP/1.1 200"), "{head}");
                let chunked = head
                    .lines()
                    .filter(|line| line.to_ascii_lowercase().starts_with("transfer-encoding:"))
                    .count();
                assert_eq!(chunked, 1, "{head}");
                let body = read_chunked(&mut client, &mut buffered).unwrap();
                assert_eq!(body, SSE_EVENT.as_bytes());
            }
            upstream.join().unwrap();
            let _ = std::fs::remove_dir_all(root);
        }

        #[test]
        fn an_unreachable_upstream_answers_502() {
            let port = {
                let listener = TcpListener::bind("127.0.0.1:0").unwrap();
                listener.local_addr().unwrap().port()
            };
            let (service, root) = fixture(format!("http://127.0.0.1:{port}/mcp"));
            let mut client = connect(&service);
            post(&mut client, &service, "{\"jsonrpc\":\"2.0\",\"id\":1}");
            let mut buffered = Vec::new();
            let head = read_head(&mut client, &mut buffered);
            assert!(head.starts_with("HTTP/1.1 502"), "{head}");
            let length = head
                .lines()
                .filter_map(|line| line.split_once(':'))
                .find(|(name, _)| name.trim().eq_ignore_ascii_case("content-length"))
                .and_then(|(_, value)| value.trim().parse::<usize>().ok())
                .unwrap();
            let body = read_exact_body(&mut client, &mut buffered, length).unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert!(json.get("error").is_some(), "{json}");
            let _ = std::fs::remove_dir_all(root);
        }
    }
}
