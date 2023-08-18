//! Utility functions for the server
//! 
//! These are used internally, but can be used externally as well.


use std::{
    path, 
    error::Error,
    fs
};

use crate::errors;
use crate::server::{
    Sendable,
    Page,
    Bytes,
    Handler,
    RequestInfo,
    ConnectionInfo,
    ConnectionType
};

use regex::Regex;
use tokio::io::{
    BufReader,
    AsyncBufReadExt,
    AsyncWriteExt,
};

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

pub async fn handle_connection(conn: ConnectionInfo, routes: Vec<Handler>, blacklisted_paths: Vec<path::PathBuf>) -> Result<(), Box<dyn Error>> {
    match conn.connection_type() {
        ConnectionType::Http => {
            handle_http_connection(conn, routes, blacklisted_paths).await?;
        },
        ConnectionType::Https => {
            handle_https_connection(conn, routes, blacklisted_paths).await?;
        }
    }
    Ok(())
}

async fn handle_http_connection(mut conn: ConnectionInfo, routes: Vec<Handler>, blacklisted_paths: Vec<path::PathBuf>) -> Result<(), Box<dyn Error>> {
    let buf_reader = BufReader::new(conn.stream());
    let request_line = match buf_reader.lines().next_line().await? {
        Some(line) => line,
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

    let request_info = RequestInfo::new(&conn, route, &blacklisted_paths);

    let mut response: Box<dyn Sendable> = Box::new(Page::new(404, String::from("Not found")));
    for handler in &routes {
        if handler.route() == route {
            response = (handler.handler())(&request_info);
            break;
        } else if handler.route() == "404" {
            response = (handler.handler())(&request_info);
        }
    }

    response.send(&mut conn).await?;
    conn.stream().flush().await?;
    Ok(())
}

async fn handle_https_connection(mut conn: ConnectionInfo, routes: Vec<Handler>, blacklisted_paths: Vec<path::PathBuf>) -> Result<(), Box<dyn Error>> {
    let buf_reader = BufReader::new(conn.ssl_stream());
    let request_line = match buf_reader.lines().next_line().await? {
        Some(line) => line,
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

    let route = &*urlencoding::decode(route)?.into_owned();

    let route = &*Regex::new(r"/\.\./")?.replace_all(route, "/").into_owned();
    // Regex replace to remove query string
    let route = &*Regex::new(r"\?[^ ]+")?.replace(route, "").into_owned();

    let request_info = RequestInfo::new(&conn, route, &blacklisted_paths);

    let mut response: Box<dyn Sendable> = Box::new(Page::new(404, String::from("Not found")));
    for handler in &routes {
        if handler.route() == route {
            response = (handler.handler())(&request_info);
            break;
        } else if handler.route() == "404" {
            response = (handler.handler())(&request_info);
        }
    }

    response.send(&mut conn).await?;
    conn.stream().flush().await?;

    Ok(())
}

pub fn base_file_handler(request: &RequestInfo) -> Box<dyn Sendable> {
    // This handles files based on route
    match request.conn.connection_type() {
        ConnectionType::Http => {
            handle_http_file(request)
        },
        ConnectionType::Https => {
            handle_https_file(request)
        }
    }
}

fn handle_http_file(request: &RequestInfo) -> Box<dyn Sendable> {
    Box::new(Bytes::new(200, &request.route[1..]).unwrap())
}

fn handle_https_file(request: &RequestInfo) -> Box<dyn Sendable> {
    Box::new(Bytes::new(200, &request.route).unwrap())
}

pub fn base_not_found_handler(request: &RequestInfo) -> Box<dyn Sendable> {
    // Check if it is a file that can be opened
    if let Ok(bytes) = Bytes::new(200, &request.route[1..]) {
        for path in request.blacklisted_paths {
            if path == bytes.file_location() {
                return Box::new(Page::new(403, String::from("Forbidden")));
            }
        }
        println!("Sending file: {}", bytes.file_location().to_str().unwrap());
        Box::new(bytes)
    } else {
        let content = fs::read_to_string("404.html").unwrap();
        Box::new(Page::new(404, content))
    }
}