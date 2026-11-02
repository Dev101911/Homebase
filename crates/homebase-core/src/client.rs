//! A blocking client for the control socket.

use crate::protocol::{Request, Response};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

pub fn connect(socket: &Path) -> std::io::Result<UnixStream> {
    let stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(Duration::from_secs(60)))?;
    Ok(stream)
}

pub fn send(stream: &mut UnixStream, request: &Request) -> std::io::Result<Response> {
    let mut line = serde_json::to_string(request).map_err(std::io::Error::other)?;
    line.push('\n');
    stream.write_all(line.as_bytes())?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut reply = String::new();
    if reader.read_line(&mut reply)? == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "the agent closed the connection",
        ));
    }
    serde_json::from_str(&reply).map_err(std::io::Error::other)
}

/// Connects, sends one request, returns the response.
pub fn request(socket: &Path, request: &Request) -> std::io::Result<Response> {
    let mut stream = connect(socket)?;
    send(&mut stream, request)
}
