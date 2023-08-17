//! Utility functions for the server
//! 
//! These are used internally, but can be used externally as well.


use std::{
    io::{
        prelude::*,
        BufReader,
        BufRead
    },
    path, 
    error::Error,
    fs::File,
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

pub fn handle_connection(conn: ConnectionInfo, routes: Vec<Handler>, blacklisted_paths: Vec<path::PathBuf>) -> Result<(), Box<dyn Error>> {
    match conn.connection_type() {
        ConnectionType::Http => {
            handle_http_connection(conn, routes, blacklisted_paths)
        },
        ConnectionType::Https(_, _) => {
            handle_https_connection(conn, routes, blacklisted_paths)
        }
    }
}

fn handle_http_connection(mut conn: ConnectionInfo, routes: Vec<Handler>, blacklisted_paths: Vec<path::PathBuf>) -> Result<(), Box<dyn Error>> {
    let buf_reader = BufReader::new(conn.stream());
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

    response.send(&mut conn)?;
    conn.stream().flush()?;
    Ok(())
}

fn handle_https_connection(mut conn: ConnectionInfo, routes: Vec<Handler>, blacklisted_paths: Vec<path::PathBuf>) -> Result<(), Box<dyn Error>> {
    let buf_reader = BufReader::new(conn.ssl_stream());
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

    response.send(&mut conn)?;
    conn.stream().flush()?;

    Ok(())
}

pub fn base_file_handler(request: &RequestInfo) -> Box<dyn Sendable> {
    // This handles files based on route
    match request.conn.connection_type() {
        ConnectionType::Http => {
            handle_http_file(request)
        },
        ConnectionType::Https(_, _) => {
            handle_https_file(request)
        }
    }
}

fn handle_http_file(request: &RequestInfo) -> Box<dyn Sendable> {
    let mut file = match File::open(&request.route[1..]) {
        Ok(file) => file,
        Err(_) => {
            return Box::new(Page::new(404, String::from("Not found")));
        }
    };
    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();
    Box::new(Bytes::new(200, contents).unwrap())
}

fn handle_https_file(request: &RequestInfo) -> Box<dyn Sendable> {
    let mut file = match File::open(&request.route[1..]) {
        Ok(file) => file,
        Err(_) => {
            return Box::new(Page::new(404, String::from("Not found")));
        }
    };
    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();
    Box::new(Bytes::new(200, contents).unwrap())
}