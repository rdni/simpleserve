//! Utility functions for the server
//! 
//! These are used internally, but can be used externally as well.


use std::{
    net::{
        TcpStream,
        SocketAddr,
        SocketAddrV4,
        Ipv4Addr
    },
    io::{
        prelude::*,
        BufReader,
        BufRead
    },
    path, error::Error,
};

use crate::errors;
use crate::server::{
    Sendable,
    Page,
    Bytes,
    Handler,
    RequestInfo
};

use regex::Regex;

pub fn get_mime_type(extension: &str) -> &'static str {
    match extension {
        "html" => "text/html",
        "css" => "text/css",
        "js" => "application/javascript",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream"
    }
}

pub fn handle_connection(mut stream: TcpStream, routes: Vec<Handler>, blacklisted_paths: Vec<path::PathBuf>) -> Result<(), Box<dyn Error>> {
    let buf_reader = BufReader::new(&mut stream);
    let request_line = match buf_reader.lines().next() {
        Some(line) => line?,
        None => {
            println!("No request line found");
            return Err(Box::new(errors::OptionUnwrapError {}));
        }
    };

    let route = match request_line.split_whitespace().nth(1) {
        Some(route) => route,
        None => {
            println!("No route found");
            return Err(Box::new(errors::OptionUnwrapError {}));
        }
    };
    // URL decode
    let route = &*urlencoding::decode(route)?.into_owned();
    // Remove /../
    let route = &*Regex::new(r"/\.\./")?.replace_all(route, "/").into_owned();
    // Regex replace to remove query string
    let route = &*Regex::new(r"\?[^ ]+")?.replace(route, "").into_owned();

    let request_info = RequestInfo {
        stream: &mut stream,
        route,
        blacklisted_paths: &blacklisted_paths
    };

    let mut response: Box<dyn Sendable> = Box::new(Page::new(404, String::from("Not found")));
    for handler in &routes {
        if handler.route() == route {
            response = (handler.handler())(&request_info);
            break;
        } else if handler.route() == "404" {
            response = (handler.handler())(&request_info);
        }
    }

    response.send(&mut stream)?;
    stream.flush()?;
    Ok(())
}

pub fn base_file_handler(request: &RequestInfo) -> Box<dyn Sendable> {
    // This handles files based on route
    println!("Request from: {} to route {}", request.stream.peer_addr().unwrap_or(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(0,0,0,0), 0))), request.route);
    if let Ok(bytes) = Bytes::new(200, &request.route[1..]) {
        for path in request.blacklisted_paths {
            if path == bytes.file_location() {
                return Box::new(Page::new(403, String::from("Forbidden")));
            }
        }
        Box::new(bytes)
    } else {
        Box::new(Page::new(404, String::from("Not found")))
    }
}