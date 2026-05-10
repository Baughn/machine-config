use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::os::unix::net::UnixStream;

use crate::api::paths;
use crate::api::types::{event_body, BuildEvent};
use crate::util::invalid;

pub fn send_event((endpoint, event): (String, BuildEvent)) -> io::Result<()> {
    let path = if event.kind == "start" {
        paths::EVENT_BUILD_START
    } else {
        paths::EVENT_BUILD_FINISH
    };
    let body = event_body(&event);
    let _ = post(&endpoint, path, &body)?;
    Ok(())
}

pub fn post(endpoint: &str, path: &str, body: &str) -> io::Result<String> {
    let request = format!(
        "POST {path} HTTP/1.1\r\nhost: nix-build-balancer\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );

    if let Some(socket) = endpoint.strip_prefix("unix:") {
        let mut stream = UnixStream::connect(socket)?;
        stream.write_all(request.as_bytes())?;
        read_http_body(stream)
    } else if let Some(addr) = endpoint.strip_prefix("tcp:") {
        let mut stream = TcpStream::connect(addr)?;
        stream.write_all(request.as_bytes())?;
        read_http_body(stream)
    } else {
        invalid("endpoint must start with unix: or tcp:")
    }
}

pub fn get_http_tcp(addr: &str, path: &str) -> io::Result<String> {
    let mut stream = TcpStream::connect(addr)?;
    let request = format!("GET {path} HTTP/1.1\r\nhost: {addr}\r\nconnection: close\r\n\r\n");
    stream.write_all(request.as_bytes())?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    let Some((headers, body)) = response.split_once("\r\n\r\n") else {
        return invalid("invalid HTTP response");
    };
    if headers.starts_with("HTTP/1.1 200 ") {
        Ok(body.to_string())
    } else {
        Err(io::Error::other(response))
    }
}

fn read_http_body<R: Read>(mut stream: R) -> io::Result<String> {
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    let Some((headers, body)) = response.split_once("\r\n\r\n") else {
        return invalid("invalid HTTP response");
    };
    if headers.starts_with("HTTP/1.1 200 ") {
        Ok(body.to_string())
    } else {
        Err(io::Error::other(response))
    }
}
