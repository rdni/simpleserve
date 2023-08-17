//! # The server module
//! 
//! This module contains a premade, multi-threaded webserver that is simple to use.
//! 
//! ## Example
//! ```
//! use sserve::server::{
//!     Webserver,
//!     Page,
//!     Sendable,
//!     HandlerFunction,
//!     RequestInfo
//! };
//! 
//! fn main() {
//!     let not_found: HandlerFunction = |_: &RequestInfo| -> Box<dyn Sendable + 'static> {
//!          Box::new(Page::new(404, String::from("Not Found")))
//!     };
//!     let main_route: HandlerFunction = |_: &RequestInfo| -> Box<dyn Sendable + 'static> {
//!          Box::new(Page::new(200, String::from("Hello World!")))
//!     };
//!     let mut server = Webserver::new(10, vec![], not_found);
//!     server.add_route("/", main_route);
//!     // server.start("127.0.0.1:7878");
//! }

use std::{
    net::{
        TcpListener,
        TcpStream,
    },
    io::prelude::*,
    path::{self, Path},
    fs::File
};

use crate::{ThreadPool, utils};

pub mod prelude {
    pub use crate::server::{
        Webserver,
        Page,
        Bytes,
        Sendable,
        Handler,
    };
}

pub trait Sendable: Send + Sync { // Can't have Sized, but should have it regardless
    fn render(&self) -> String;
    fn send(&self, stream: &mut TcpStream) -> std::io::Result<()> {
        stream.write_all(self.render().as_bytes())?;
        Ok(())
    }
}

/// A handler function
/// 
/// # Arguments
/// * `request` - The request info
pub type HandlerFunction = fn(&RequestInfo) -> Box<dyn Sendable>;

/// The webserver
/// 
/// # Examples
/// ```
/// use sserve::server::{Webserver, Page, Sendable};
/// 
/// fn main() {
///     let mut server = Webserver::new(10, vec![], |_| -> Box<dyn Sendable> {
///         Box::new(Page::new(404, String::from("Not found")))
/// });
///     // server.start("127.0.0.1:7878");
/// }
/// ```
pub struct Webserver {
    routes: Vec<Handler>,
    thread_pool: ThreadPool,
    blacklisted_paths: Vec<path::PathBuf>,
}

impl Webserver {
    /// Creates a new webserver
    /// 
    /// # Arguments
    /// * `thread_amount` - The number of threads to use
    /// * `blacklisted_paths` - The paths (file paths) to not allow access to
    /// * `not_found_handler` - The handler for 404 errors
    pub fn new(thread_amount: usize, blacklisted_paths: Vec<path::PathBuf>, not_found_handler: HandlerFunction) -> Webserver {
        Webserver {
            routes: vec![Handler::new("404", not_found_handler)],
            thread_pool: ThreadPool::new(thread_amount),
            blacklisted_paths
        }
    }

    pub fn blacklisted_paths(&self) -> &Vec<path::PathBuf> {
        &self.blacklisted_paths
    }

    /// Adds a route to the webserver
    /// 
    /// # Arguments
    /// * `route` - The route to add
    /// * `handler` - The handler for the route
    /// 
    /// # Panics
    /// Panics if the route is empty
    /// 
    /// # Examples
    /// ```
    /// use std::{
    ///     net::TcpStream,
    ///     fs,
    ///     path::PathBuf
    /// };
    /// use sserve::server::{
    ///     Webserver,
    ///     Page,
    ///     Sendable,
    ///     RequestInfo
    /// };
    ///
    /// fn main() {
    ///     let mut server = Webserver::new(10, vec![], not_found);
    ///     server.add_route("/", main_route);
    ///     // server.start("127.0.0.1:7878");
    /// }
    ///
    ///
    /// fn main_route(request: &RequestInfo) -> Box<dyn Sendable> {
    ///     let contents = fs::read_to_string("index.html").expect("Error reading file");
    ///     Box::new(Page::new(200, contents))
    /// }
    /// 
    /// fn not_found(_: &RequestInfo) -> Box<dyn Sendable> {
    ///     Box::new(Page::new(404, String::from("Not found")))
    /// }
    pub fn add_route(&mut self, route: &str, handler: HandlerFunction) {
        if route.is_empty() {
            panic!("Route cannot be empty");
        }
        for route_handler in &self.routes {
            if route_handler.route == route {
                panic!("Route already exists");
            }
        }
        println!("Added route {}", route);
        self.routes.push(Handler::new(route, handler));
    }

    pub fn add_accessible_files(&mut self, paths: Vec<&str>) -> Result<(), std::io::Error> {
        for path_str in paths {
            path::Path::new(path_str).canonicalize()?;
            let path_str = &*(String::from("/") + path_str);
            println!("Added route {}", path_str);
            self.add_route(path_str, utils::base_file_handler);
        }
        Ok(())
    }

    /// Starts the webserver
    /// 
    /// # Arguments
    /// * `addr` - The address to start the server on
    /// 
    /// # Panics
    /// Panics if the address is invalid
    pub fn start(&self, addr: &str) -> ! {
        let listener = TcpListener::bind(addr).expect("Invalid address");
        println!("Server started on {}...", addr);
        for stream in listener.incoming() {
            let stream = match stream {
                Ok(v) => v,
                Err(e) => {
                    println!("Error processing TCP stream: {}", e);
                    continue;
                }
            };

            let route_clone = self.routes.clone();
            let blacklisted_paths_clone = self.blacklisted_paths.clone();
    
            self.thread_pool.execute(|| {
                if let Err(e) = utils::handle_connection(stream, route_clone, blacklisted_paths_clone) {
                    println!("Error handling connection: {}", e);
                }
            });
        }
        panic!("Server stopped");
    }
}

/// Internal handler struct
/// 
/// Cannot be created outside of the library
#[derive(Clone)]
pub struct Handler {
    route: String,
    handler: HandlerFunction,
}

impl Handler {
    fn new(route: &str, handler: HandlerFunction) -> Handler {
        Handler {
            route: String::from(route),
            handler,
        }
    }
    pub fn route(&self) -> &str {
        &self.route
    }
    pub fn handler(&self) -> HandlerFunction {
        self.handler
    }
}

/// A page to be rendered
/// 
/// # Examples
/// ```
/// use std::{
///     net::TcpStream,
///     fs,
///     path::PathBuf
/// };
/// use sserve::server::{
///     Webserver,
///     Page,
///     Sendable,
///     RequestInfo
/// };
///
/// fn main() {
///     let mut server = Webserver::new(10, vec![], not_found);
///     server.add_route("/", main_route);
///     // server.start("127.0.0.1:7878");
/// }
///
///
/// fn main_route(_: &RequestInfo) -> Box<dyn Sendable> {
///     let contents = fs::read_to_string("index.html").expect("Error reading file");
///     Box::new(Page::new(200, contents))
/// }
/// 
/// fn not_found(_: &RequestInfo) -> Box<dyn Sendable> {
///     Box::new(Page::new(404, String::from("Not found")))
/// }
pub struct Page {
    status: u16,
    content: String,
}

impl Page {
    pub fn new(status: u16, content: String) -> Page {
        Page {
            status,
            content,
        }
    }
}

impl Sendable for Page {
    fn render(&self) -> String {
        format!("HTTP/1.1 {} OK\r\nContent-Length: {}\r\n\r\n{}", self.status, self.content.len(), self.content)
    }
}


/// A file to be rendered
/// 
/// # Examples
/// ```
/// use std::{
///     net::TcpStream,
///     fs,
///     path::PathBuf
/// };
/// use sserve::server::{
///     Webserver,
///     Bytes,
///     Sendable,
///     RequestInfo,
///     Page
/// };
/// 
/// fn main() {
///    let mut server = Webserver::new(10, vec![], not_found);
///    server.add_route("/", main_route);
///    server.add_route("/image.jpg", image_route);
///    // server.start("127.0.0.1:7878");
/// }
/// 
/// fn main_route(_: &RequestInfo) -> Box<dyn Sendable> {
///    let contents = fs::read_to_string("index.html").expect("Error reading file");
///    Box::new(Page::new(200, contents))
/// }
/// 
/// fn image_route(_: &RequestInfo) -> Box<dyn Sendable> {
///    let bytes = Bytes::new(200, "image.jpg").expect("Error reading file");
///    Box::new(bytes)
/// }
/// 
/// fn not_found(_: &RequestInfo) -> Box<dyn Sendable> {
///    Box::new(Page::new(404, String::from("Not found")))
/// }
/// ```
pub struct Bytes {
    status: u16,
    content: Vec<u8>,
    file_location: path::PathBuf,
    file_type: String,
}

impl Bytes {
    pub fn new<P: AsRef<Path>>(status: u16, path: P) -> Result<Bytes, std::io::Error> {
        let canonical_path = path::Path::new(path.as_ref()).canonicalize()?;
        let mut file = File::open(path)?;
        let mut content = Vec::new();
        file.read_to_end(&mut content)?;
        let file_type = match canonical_path.extension() {
            Some(v) => v.to_str().unwrap_or(""),
            None => "",
        };
        Ok(Bytes {
            status,
            content,
            file_type: String::from(file_type),
            file_location: canonical_path,
        })
    }

    pub fn file_location(&self) -> &path::PathBuf {
        &self.file_location
    }
}

impl Sendable for Bytes {
    fn render(&self) -> String {
        format!(
            "HTTP/1.1 {} OK\r\nContent-Type: {}\r\nContent-Length: {}\r\n\r\n",
            self.status,
            utils::get_mime_type(&self.file_type),
            self.content.len()
        )
    }

    fn send(&self, stream: &mut TcpStream) -> std::io::Result<()> {
        stream.write_all(self.render().as_bytes())?;
        stream.write_all(&self.content)?;
        Ok(())
    }
}

pub struct RequestInfo<'a> {
    pub stream: &'a TcpStream,
    pub route: &'a str,
    pub blacklisted_paths: &'a Vec<path::PathBuf>,
}

impl<'a> RequestInfo<'a> {
    pub fn new(stream: &'a TcpStream, route: &'a str, blacklisted_paths: &'a Vec<path::PathBuf>) -> RequestInfo<'a> {
        RequestInfo {
            stream,
            route,
            blacklisted_paths,
        }
    }
}