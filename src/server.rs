//! # The server module
//! 
//! This module contains a premade, multi-threaded webserver that is simple to use.
//! 
//! ## Example
//! ```
//! use simpleserve::server::{
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

use openssl::ssl::{
        SslAcceptor,
        SslMethod,
        SslFiletype,
        SslStream
    };
use std::{
    net::{
        TcpListener,
        TcpStream,
    },
    io::prelude::*,
    path::{
        self, 
        Path, 
        PathBuf
    },
    fs::File,
    error::Error,
    sync::mpsc::{
        self, 
        Receiver
    },
};

use crate::{
    ThreadPool, 
    utils,
    Job
};

pub mod prelude {
    pub use crate::server::{
        Webserver,
        Page,
        Bytes,
        Sendable,
        Handler,
        RequestInfo,
        ConnectionInfo,
        ConnectionType
    };
    pub use crate::utils::{
        get_mime_type,
        base_not_found_handler
    };
}

pub trait Sendable: Send + Sync {
    fn render(&self) -> String;
    fn send(&self, conn: &mut ConnectionInfo) -> Result<(), std::io::Error> {
        match conn.connection_type() {
            ConnectionType::Http => {
                conn.stream().write_all(self.render().as_bytes())?;
                return Ok(());
            },
            ConnectionType::Https(_, _) => {
                conn.ssl_stream().write_all(self.render().as_bytes())?;
                return Ok(());
            }
        }
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
/// use simpleserve::server::{Webserver, Page, Sendable};
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
    connection_type: Option<ConnectionType>,
    whitelist_enabled: bool,
    receiver: Option<mpsc::Receiver<Job>>,
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
            blacklisted_paths,
            connection_type: None,
            whitelist_enabled: false,
            receiver: None,
        }
    }

    pub fn blacklisted_paths(&self) -> &Vec<path::PathBuf> {
        &self.blacklisted_paths
    }

    pub fn connection_type(&self) -> &Option<ConnectionType> {
        &self.connection_type
    }

    pub fn whitelist_enabled(&self) -> bool {
        self.whitelist_enabled
    }

    pub fn set_whitelist_enabled(&mut self, enabled: bool) {
        self.whitelist_enabled = enabled;
    }

    pub fn with_receiver(mut self, receiver: mpsc::Receiver<Job>) -> Webserver {
        self.receiver = Some(receiver);
        self
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
    /// use simpleserve::server::{
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
    pub fn start(&mut self, addr: &str, connection_type: ConnectionType) -> Result<(), Box<dyn Error>> {
        if let ConnectionType::Http = connection_type {
            self.connection_type = Some(connection_type);
            self.start_http(addr)?;
        } else {
            self.connection_type = Some(connection_type);
            self.start_https(addr)?;
        }
        Ok(())
    }

    fn start_http(&self, addr: &str) -> Result<(), Box<dyn Error>> {
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
    
            let connection_info = ConnectionInfo::new(stream);

            self.thread_pool.execute(|| {
                if let Err(e) = utils::handle_connection(connection_info, route_clone, blacklisted_paths_clone) {
                    println!("Error handling connection: {}", e);
                }
            });
        }
        Ok(())
    }

    fn start_https(&self, addr: &str) -> Result<(), Box<dyn Error>> {
        let mut acceptor_builder = SslAcceptor::mozilla_intermediate(SslMethod::tls()).unwrap();
        acceptor_builder.set_private_key_file("", SslFiletype::PEM).unwrap();
        acceptor_builder.set_certificate_chain_file("/etc/passwd",).unwrap();
        let acceptor = acceptor_builder.build();

        let listener = TcpListener::bind(addr).expect("Invalid address");
        println!("Server started on {}...", addr);
        for stream in listener.incoming() {
            let stream = acceptor.accept(stream?).expect("Failed to establish SSL connection");

            let route_clone = self.routes.clone();
            let blacklisted_paths_clone = self.blacklisted_paths.clone();

            let connection_info = ConnectionInfo::new_ssl(stream, self.connection_type.as_ref().unwrap().key_path().clone(), self.connection_type.as_ref().unwrap().certificate().clone());
    
            self.thread_pool.execute(|| {
                if let Err(e) = utils::handle_connection(connection_info, route_clone, blacklisted_paths_clone) {
                    println!("Error handling connection: {}", e);
                }
            });
        }
        Ok(())
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
/// use simpleserve::server::{
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
/// use simpleserve::server::{
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

    fn send(&self, conn: &mut ConnectionInfo) -> Result<(), std::io::Error> {
        match conn.connection_type() {
            ConnectionType::Http => {
                conn.stream().write_all(self.render().as_bytes())?;
                conn.stream().write_all(&self.content)?;
                return Ok(());
            },
            ConnectionType::Https(_, _) => {
                conn.ssl_stream().write_all(self.render().as_bytes())?;
                conn.ssl_stream().write_all(&self.content)?;
                return Ok(());
            }
        }
    }
}

pub struct RequestInfo<'a> {
    pub conn: &'a ConnectionInfo,
    pub route: &'a str,
    pub blacklisted_paths: &'a Vec<path::PathBuf>,
}

impl<'a> RequestInfo<'a> {
    pub fn new(conn: &'a ConnectionInfo, route: &'a str, blacklisted_paths: &'a Vec<path::PathBuf>) -> RequestInfo<'a> {
        RequestInfo {
            conn,
            route,
            blacklisted_paths,
        }
    }
}

pub enum ConnectionType {
    Http,
    Https(
        PathBuf,
        PathBuf,
    ),
}

impl ConnectionType {
    pub fn key_path(&self) -> &PathBuf {
        match self {
            ConnectionType::Http => panic!("Connection is not HTTPS"),
            ConnectionType::Https(key_path, _) => key_path,
        }
    }

    pub fn certificate(&self) -> &PathBuf {
        match self {
            ConnectionType::Http => panic!("Connection is not HTTPS"),
            ConnectionType::Https(_, certificate) => certificate,
        }
    }
}

pub struct ConnectionInfo {
    connection_type: ConnectionType,
    ssl_stream: Option<SslStream<TcpStream>>,
    stream: Option<TcpStream>,
}

impl ConnectionInfo {
    pub fn new(stream: TcpStream) -> ConnectionInfo {
        ConnectionInfo {
            connection_type: ConnectionType::Http,
            ssl_stream: None,
            stream: Some(stream),
        }
    }

    pub fn new_ssl(stream: SslStream<TcpStream>, key_path: PathBuf, certificate: PathBuf) -> ConnectionInfo {
        ConnectionInfo {
            connection_type: ConnectionType::Https(key_path, certificate),
            ssl_stream: Some(stream),
            stream: None,
        }
    }

    pub fn stream(&mut self) -> &mut TcpStream {
        match &mut self.stream {
            Some(v) => v,
            None => panic!("Connection is not HTTP"),
        }
    }

    pub fn ssl_stream(&mut self) -> &mut SslStream<TcpStream> {
        match &mut self.ssl_stream {
            Some(v) => v,
            None => panic!("Connection is not HTTPS"),
        }
    }

    pub fn connection_type(&self) -> &ConnectionType {
        &self.connection_type
    }
}