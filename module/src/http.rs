//! The enclave's way out: `wasi:http`, through the worker's allowlist.
//!
//! Every call keeps the exact bytes it sent and received. Hashes and signatures
//! later in the run are taken over those bytes, so nothing here parses,
//! normalises or re-encodes a body.
//!
//! Written on `wasi:http` directly rather than through a client crate: an
//! output stream takes at most what `check-write` grants (4096 bytes on the
//! worker), so a body goes out in as many pieces as the stream asks for —
//! after the request is handed over, so the host is already draining it.

use wasi::http::outgoing_handler;
use wasi::http::types::{ErrorCode, Fields, IncomingBody, Method, OutgoingBody, OutgoingRequest, RequestOptions, Scheme};
use wasi::io::streams::{InputStream, OutputStream, StreamError};

const CONNECT_TIMEOUT_NS: u64 = 10_000_000_000;
const READ_CHUNK: u64 = 64 * 1024;

/// An answer as it arrived: status and body.
pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// The body as text, cut to what an error message can carry.
    pub fn excerpt(&self) -> String {
        String::from_utf8_lossy(&self.body).trim().chars().take(300).collect()
    }
}

pub fn post(url: &str, headers: &[(&str, &str)], body: &[u8]) -> Result<Reply, String> {
    send(Method::Post, url, headers, Some(body))
}

fn send(method: Method, url: &str, headers: &[(&str, &str)], body: Option<&[u8]>) -> Result<Reply, String> {
    let host_name = host(url);
    let refused = |what: &str| format!("the request to {host_name} could not be built: {what}");
    let path = url
        .strip_prefix("https://")
        .ok_or_else(|| refused("only https URLs are used"))?
        .get(host_name.len()..)
        .filter(|p| !p.is_empty())
        .unwrap_or("/");

    let fields = Fields::new();
    for (name, value) in headers {
        fields.append(&name.to_string(), &value.as_bytes().to_vec()).map_err(|e| refused(&format!("header {name}: {e:?}")))?;
    }
    if let Some(body) = body {
        fields.append(&"Content-Length".to_string(), &body.len().to_string().into_bytes()).map_err(|e| refused(&format!("{e:?}")))?;
    }
    let request = OutgoingRequest::new(fields);
    request.set_method(&method).map_err(|()| refused("method"))?;
    request.set_scheme(Some(&Scheme::Https)).map_err(|()| refused("scheme"))?;
    request.set_authority(Some(host_name)).map_err(|()| refused("authority"))?;
    request.set_path_with_query(Some(if path.starts_with('/') { path } else { "/" })).map_err(|()| refused("path"))?;
    let outgoing = request.body().map_err(|()| refused("body"))?;
    let options = RequestOptions::new();
    options.set_connect_timeout(Some(CONNECT_TIMEOUT_NS)).map_err(|()| refused("timeout"))?;

    let pending = outgoing_handler::handle(request, Some(options)).map_err(|e| failed(host_name, &e))?;
    if let Some(body) = body {
        let stream = outgoing.write().map_err(|()| refused("body stream"))?;
        write_all(&stream, body).map_err(|e| format!("sending the request to {host_name} failed: {e}"))?;
    }
    OutgoingBody::finish(outgoing, None).map_err(|e| failed(host_name, &e))?;

    let response = loop {
        match pending.get() {
            Some(result) => break result.map_err(|()| refused("the response was already taken"))?.map_err(|e| failed(host_name, &e))?,
            None => pending.subscribe().block(),
        }
    };
    let status = response.status();
    let incoming = response.consume().map_err(|()| refused("the response body was already taken"))?;
    let body = {
        let stream = incoming.stream().map_err(|()| refused("the response body stream"))?;
        read_all(&stream).map_err(|e| format!("the answer from {host_name} did not arrive whole: {e}"))?
    };
    drop(IncomingBody::finish(incoming));
    Ok(Reply { status, body })
}

/// Write `bytes` in the pieces the stream grants, then wait for the flush.
fn write_all(stream: &OutputStream, mut bytes: &[u8]) -> Result<(), String> {
    while !bytes.is_empty() {
        let granted = stream.check_write().map_err(stream_error)?;
        if granted == 0 {
            stream.subscribe().block();
            continue;
        }
        let n = bytes.len().min(granted as usize);
        stream.write(&bytes[..n]).map_err(stream_error)?;
        bytes = &bytes[n..];
    }
    stream.blocking_flush().map_err(stream_error)
}

fn read_all(stream: &InputStream) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    loop {
        match stream.blocking_read(READ_CHUNK) {
            Ok(chunk) => body.extend_from_slice(&chunk),
            Err(StreamError::Closed) => return Ok(body),
            Err(e) => return Err(stream_error(e)),
        }
    }
}

fn stream_error(e: StreamError) -> String {
    match e {
        StreamError::Closed => "the stream closed early".to_string(),
        StreamError::LastOperationFailed(err) => err.to_debug_string(),
    }
}

/// The host's refusal, said so that a timeout reads as one.
fn failed(host_name: &str, e: &ErrorCode) -> String {
    match e {
        ErrorCode::ConnectionTimeout | ErrorCode::ConnectionReadTimeout | ErrorCode::HttpResponseTimeout => format!(
            "{host_name} did not answer in time ({e:?}): the manifest gives NEAR AI 120 s a request"
        ),
        _ => format!("{host_name} could not be reached: {e:?}"),
    }
}

/// The host of a URL, for messages: the path may carry query values nobody
/// needs to read in an error.
pub fn host(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.split(['/', '?']).next().unwrap_or(rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_is_named_without_its_path_or_query() {
        assert_eq!(host("https://cloud-api.near.ai/v1/systemone?x=1"), "cloud-api.near.ai");
        assert_eq!(host("https://cloud-api.near.ai?x"), "cloud-api.near.ai");
    }
}
