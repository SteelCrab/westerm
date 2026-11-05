//! One HTTP/1.1 request over any byte stream: to the PTY host's socket, or
//! over a link to a server's dashboard.

use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// How long one request may take, from the first byte sent to the last read.
const EXCHANGE_TIMEOUT: Duration = Duration::from_secs(10);

/// Sends one request on `stream` and reads the answer to the end: (status,
/// body). A body is sent as JSON. None when the peer fails, closes without
/// an answer, or takes longer than EXCHANGE_TIMEOUT.
pub(super) async fn exchange<S>(mut stream: S, method: &str, path: &str, body: Option<&[u8]>) -> Option<(u16, Vec<u8>)>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let work = async {
        let mut request = format!("{method} {path} HTTP/1.1\r\nHost: westerm\r\nConnection: close\r\n");
        match body {
            Some(body) => request.push_str(&format!("Content-Type: application/json\r\nContent-Length: {}\r\n\r\n", body.len())),
            None => request.push_str("Content-Length: 0\r\n\r\n"),
        }
        stream.write_all(request.as_bytes()).await.ok()?;
        stream.write_all(body.unwrap_or_default()).await.ok()?;
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.ok()?;
        parse_response(&response)
    };
    tokio::time::timeout(EXCHANGE_TIMEOUT, work).await.ok().flatten()
}

/// Status and body of an HTTP/1.1 response read to the end. Handles chunked
/// bodies: Node writes them when it sends no Content-Length, and the
/// Node-era host answers /health that way.
pub(super) fn parse_response(raw: &[u8]) -> Option<(u16, Vec<u8>)> {
    let split = raw.windows(4).position(|window| window == b"\r\n\r\n")?;
    let head = String::from_utf8_lossy(&raw[..split]);
    let body = &raw[split + 4..];
    let status = head.split_whitespace().nth(1)?.parse().ok()?;
    let chunked = head
        .lines()
        .any(|line| line.to_ascii_lowercase().replace(' ', "") == "transfer-encoding:chunked");
    if !chunked {
        return Some((status, body.to_vec()));
    }
    let mut decoded = Vec::new();
    let mut rest = body;
    loop {
        let line_end = rest.windows(2).position(|window| window == b"\r\n")?;
        let size_line = std::str::from_utf8(&rest[..line_end]).ok()?;
        let size = usize::from_str_radix(size_line.split(';').next()?.trim(), 16).ok()?;
        let after = &rest[line_end + 2..];
        if size == 0 {
            return Some((status, decoded));
        }
        decoded.extend_from_slice(after.get(..size)?);
        rest = after.get(size..)?.strip_prefix(b"\r\n")?;
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::{exchange, parse_response};

    /// Reads from `stream` until the bytes so far end with `end`.
    async fn read_until<S: AsyncReadExt + Unpin>(stream: &mut S, end: &[u8]) -> Vec<u8> {
        let mut seen = Vec::new();
        while !seen.ends_with(end) {
            let mut chunk = [0u8; 256];
            let n = stream.read(&mut chunk).await.unwrap();
            assert!(n > 0, "the stream ended before {end:?}; got {seen:?}");
            seen.extend_from_slice(&chunk[..n]);
        }
        seen
    }

    #[tokio::test]
    async fn exchange_sends_one_request_and_reads_to_the_end() {
        let (client, mut server) = tokio::io::duplex(4096);
        let answer = tokio::spawn(async move {
            let request = read_until(&mut server, b"[]").await; // The head and the body.
            server.write_all(b"HTTP/1.1 204 No Content\r\n\r\n").await.unwrap();
            String::from_utf8(request).unwrap()
        });
        assert_eq!(exchange(client, "PUT", "/api/workspace", Some(b"[]")).await, Some((204, Vec::new())));
        let request = answer.await.unwrap();
        assert!(request.starts_with("PUT /api/workspace HTTP/1.1\r\n"));
        assert!(request.contains("Content-Length: 2\r\n") && request.contains("Connection: close\r\n"));
    }

    #[test]
    fn reads_plain_and_chunked_responses() {
        let plain = b"HTTP/1.1 200 OK\r\nContent-Length: 13\r\n\r\n{\"build\":\"x\"}";
        assert_eq!(parse_response(plain), Some((200, br#"{"build":"x"}"#.to_vec())));
        // What the Node host sends for res.end(JSON.stringify(...)).
        let chunked = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n26\r\n{\"build\":\"8ff41cb4215e\",\"pid\":1,\"x\":2}\r\n0\r\n\r\n";
        assert_eq!(parse_response(chunked), Some((200, br#"{"build":"8ff41cb4215e","pid":1,"x":2}"#.to_vec())));
        assert_eq!(parse_response(b"HTTP/1.1 204 No Content\r\n\r\n"), Some((204, Vec::new())));
        assert_eq!(parse_response(b"garbage"), None);
    }
}
