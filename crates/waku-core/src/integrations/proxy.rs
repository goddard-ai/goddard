//! The local MCP proxy. Agents connect to `http://127.0.0.1:<port>/mcp/<id>`
//! with the per-install bearer token; the proxy injects the real upstream
//! credential and relays the request through `curl`, streaming the response
//! (SSE included) back verbatim.
//!
//! Plain HTTP/1.1, thread per connection, keep-alive supported because
//! streamable-HTTP clients reuse connections.

use std::io::{Read as _, Write as _};
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
    let authorized = request
        .headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("authorization"))
        .is_some_and(|(_, v)| v == &format!("Bearer {}", inner.proxy_token()));
    if !authorized {
        write_simple(stream, 401, "missing or invalid proxy token");
        return Ok(());
    }
    let id = request
        .path
        .strip_prefix("/mcp/")
        .map(|rest| rest.trim_end_matches('/').to_owned())
        .filter(|rest| !rest.is_empty() && !rest.contains('/'));
    let Some(id) = id else {
        write_simple(stream, 404, "unknown integration endpoint");
        return Ok(());
    };

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
    if let Some(auth) = &upstream.auth_header {
        headers.push(("Authorization", auth.clone()));
    }
    let job = CurlJob {
        method: &request.method,
        url: &upstream.url,
        headers: &headers,
        body_file: body_file.as_deref(),
        follow: false,
    };
    let mut child = match http::spawn_with(&job, std::process::Stdio::null()) {
        Ok(child) => child,
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
    let result = std::io::copy(&mut stdout, stream);
    let _ = child.kill();
    let _ = child.wait();
    // curl reads the body file during the request; remove it only once the
    // child is done.
    if let Some(path) = &body_file {
        let _ = std::fs::remove_file(path);
    }
    result?;
    Ok(())
}

fn write_simple(stream: &mut TcpStream, status: u16, message: &str) {
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
}
