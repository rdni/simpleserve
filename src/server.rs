//! # The server module
//! 
//! This module contains a premade, multi-threaded webserver that is simple to use.
//! 
//! ## Example
//! ```
//! use simpleserve::{
//!     Webserver,
//!     Page,
//!     Sendable,
//!     HandlerFunction,
//!     RequestInfo,
//!     ConnectionType
//! };
//! 
//! fn main() {
//!     let main_route: HandlerFunction = |_: &RequestInfo| -> Box<dyn Sendable + 'static> {
//!          Box::new(Page::new(200, String::from("Hello World!")))
//!     };
//!     let mut server = Webserver::new(10, vec![]);
//!     server.add_route("/", main_route);
//!     server.start("127.0.0.1:7878", ConnectionType::Http, None, None);
//! }

use openssl::ssl::{
    SslAcceptor,
    SslFiletype,
    SslMethod,
    Ssl,
};
use tokio_openssl::SslStream;
use std::{
    io::prelude::*,
    path::{
        self, 
        Path, 
        PathBuf
    },
    fs::File,
    error::Error,
    thread,
    time::Duration,
};

use crate::{
    ThreadPool, 
    utils
};

use tokio::{
    self,
    sync::mpsc,
    net::{
        TcpListener,
        TcpStream
    },
    io::AsyncWriteExt,
    runtime::Runtime,
};

use async_trait::async_trait;

pub mod prelude {
    pub use crate::server::{
        Webserver,
        Page,
        Bytes,
        Sendable,
        Handler,
        RequestInfo,
        ConnectionInfo,
        ConnectionType,
        Task,
        HandlerFunction
    };
    pub use crate::utils::{
        get_mime_type,
        base_not_found_handler
    };
}

#[async_trait]
pub trait Sendable: Send + Sync {
    fn render(&self) -> String;
    async fn send(&self, conn: &mut ConnectionInfo) -> Result<(), std::io::Error> {
        // Runtime already created in handle_connection, just use that
        match conn.connection_type() {
            ConnectionType::Http => {
                conn.stream().write_all(self.render().as_bytes()).await?;
                return Ok(());
            },
            ConnectionType::Https => {
                conn.ssl_stream().write_all(self.render().as_bytes()).await?;
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
/// use simpleserve::{
///     Webserver,
///     ConnectionType
/// };
/// 
/// fn main() {
///     let mut server = Webserver::new(10, vec![]);
///     server.start("127.0.0.1:7878", ConnectionType::Http, None, None);
/// }
/// ```
pub struct Webserver {
    routes: Vec<Handler>,
    thread_pool: ThreadPool,
    blacklisted_paths: Vec<path::PathBuf>,
    connection_type: Option<ConnectionType>,
    receiver: Option<mpsc::Receiver<Task>>,
}

impl Webserver {
    /// Creates a new webserver
    /// 
    /// # Arguments
    /// * `thread_amount` - The number of threads to use
    /// * `blacklisted_paths` - The paths (file paths) to not allow access to
    /// * `not_found_handler` - The handler for 404 errors
    pub fn new(thread_amount: usize, blacklisted_paths: Vec<path::PathBuf>) -> Webserver {
        Webserver {
            routes: vec![Handler::new("404", utils::base_not_found_handler)],
            thread_pool: ThreadPool::new(thread_amount),
            blacklisted_paths,
            connection_type: None,
            receiver: None,
        }
    }

    pub fn blacklisted_paths(&self) -> &Vec<path::PathBuf> {
        &self.blacklisted_paths
    }

    pub fn connection_type(&self) -> &Option<ConnectionType> {
        &self.connection_type
    }

    pub fn with_receiver(mut self, receiver: mpsc::Receiver<Task>) -> Webserver {
        self.receiver = Some(receiver);
        self
    }

    pub fn set_404_callback(&mut self, callback: HandlerFunction) {
        self.routes[0] = Handler::new("404", callback);
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
    /// use simpleserve::{
    ///     Webserver,
    ///     Page,
    ///     Sendable,
    ///     RequestInfo,
    ///     ConnectionType
    /// };
    ///
    /// fn main() {
    ///     let mut server = Webserver::new(10, vec![]);
    ///     server.add_route("/", main_route);
    ///     server.start("127.0.0.1:7878", ConnectionType::Http, None, None);
    /// }
    ///
    ///
    /// fn main_route(request: &RequestInfo) -> Box<dyn Sendable> {
    ///     let contents = fs::read_to_string("index.html").expect("Error reading file");
    ///     Box::new(Page::new(200, contents))
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

    async fn receive(&mut self) -> Option<Task> {
        match self.receiver.take() {
            Some(mut receiver) => {
                match receiver.recv().await {
                    Some(message) => {
                        self.receiver = Some(receiver);
                        return Some(message);
                    },
                    None => {
                        println!("Receiver channel closed");
                        return None;
                    }
                }
            },
            None => {
                return None;
            }
        }
    }

    /// Starts the webserver
    /// 
    /// # Arguments
    /// * `addr` - The address to start the server on
    /// 
    /// # Panics
    /// Panics if the address is invalid
    pub async fn start(&mut self, addr: &str, connection_type: ConnectionType, pk: Option<PathBuf>, sslc: Option<PathBuf>) -> Result<(), Box<dyn Error>> {
        if let ConnectionType::Http = connection_type {
            self.connection_type = Some(connection_type);
            self.start_http(addr).await?;
        } else if let ConnectionType::Https = connection_type {
            self.connection_type = Some(ConnectionType::Https);
            self.start_https(addr, pk.unwrap(), sslc.unwrap()).await?;
        }
        self.thread_pool.stop();
        Ok(())
    }

    async fn start_http(&mut self, addr: &str) -> Result<(), Box<dyn Error>> {
        let listener = TcpListener::bind(addr).await?;
        println!("Server started on {}...", addr);
        loop {
            tokio::select! {
                conn = listener.accept() => match conn {
                    Ok((stream, _)) => {
                        let route_clone = self.routes.clone();
                        let blacklisted_paths_clone = self.blacklisted_paths.clone();

                        let connection_info = ConnectionInfo::new(stream);

                        self.thread_pool.execute(|| {
                            let rt = Runtime::new().unwrap();
                            rt.block_on(
                                utils::handle_connection(connection_info, route_clone, blacklisted_paths_clone)
                            ).unwrap();
                        });
                    },
                    Err(e) => {
                        println!("Error accepting connection: {}", e);
                    }
                },
                msg = self.receive() => {
                    match msg {
                        Some(Task::Shutdown) => {
                            println!("Shutting down server...");
                            return Ok(());
                        },
                        None => {}
                        _ => {
                            println!("Received unknown message");
                        }
                    }
                }
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    async fn start_https(&self, addr: &str, private_key_file: PathBuf, ssl_certificate_file: PathBuf) -> Result<(), Box<dyn Error>> {
        let listener = TcpListener::bind(addr).await?;

        let mut acceptor_builder = SslAcceptor::mozilla_intermediate(SslMethod::tls()).unwrap();
        acceptor_builder.set_private_key_file(private_key_file, SslFiletype::PEM).unwrap();
        acceptor_builder.set_certificate_chain_file(ssl_certificate_file).unwrap();
        let acceptor = acceptor_builder.build();

        let ssl = Ssl::new(acceptor.context()).unwrap();

        tokio::select! {
            conn = listener.accept() => match conn {
                Ok((stream, _)) => {
                    let stream = SslStream::new(ssl, stream).unwrap();

                    let route_clone = self.routes.clone();
                    let blacklisted_paths_clone = self.blacklisted_paths.clone();

                    let connection_info = ConnectionInfo::new_ssl(stream);

                    self.thread_pool.execute(|| {
                        let rt = Runtime::new().unwrap();
                                    
                        rt.block_on(
                            utils::handle_connection(connection_info, route_clone, blacklisted_paths_clone)
                        ).unwrap()
                    });
                },
                Err(e) => {
                    println!("Error accepting connection: {}", e);
                }
            },
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
/// use simpleserve::{
///     Webserver,
///     Page,
///     Sendable,
///     RequestInfo,
///     ConnectionType
/// };
///
/// fn main() {
///     let mut server = Webserver::new(10, vec![]);
///     server.add_route("/", main_route);
///     let connection_type = ConnectionType::Http;
///     server.start("127.0.0.1:7878", connection_type, None, None);
/// }
///
///
/// fn main_route(_: &RequestInfo) -> Box<dyn Sendable> {
///     let contents = fs::read_to_string("index.html").expect("Error reading file");
///     Box::new(Page::new(200, contents))
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
/// use simpleserve::{
///     Webserver,
///     Bytes,
///     Sendable,
///     RequestInfo,
///     Page,
///     ConnectionType
/// };
/// 
/// fn main() {
///    let mut server = Webserver::new(10, vec![]);
///    server.add_route("/", main_route);
///    server.add_route("/image.jpg", image_route);
///    server.set_404_callback(not_found);
///    server.start("127.0.0.1:7878", ConnectionType::Http, None, None);
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

#[async_trait]
impl Sendable for Bytes {
    fn render(&self) -> String {
        format!(
            "HTTP/1.1 {} OK\r\nContent-Type: {}\r\nContent-Length: {}\r\n\r\n",
            self.status,
            utils::get_mime_type(&self.file_type),
            self.content.len()
        )
    }

    async fn send(&self, conn: &mut ConnectionInfo) -> Result<(), std::io::Error> {
        match conn.connection_type() {
            ConnectionType::Http => {
                conn.stream().write_all(self.render().as_bytes()).await?;
                conn.stream().write_all(&self.content).await?;
                return Ok(());
            },
            ConnectionType::Https => {
                conn.ssl_stream().write_all(self.render().as_bytes()).await?;
                conn.ssl_stream().write_all(&self.content).await?;
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

#[derive(Debug)]
pub enum Task {
    Connection(ConnectionInfo),
    Shutdown,
}

#[derive(Debug)]
pub enum ConnectionType {
    Http,
    Https,
}

#[derive(Debug)]
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

    pub fn new_ssl(stream: SslStream<TcpStream>) -> ConnectionInfo {
        ConnectionInfo {
            connection_type: ConnectionType::Https,
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