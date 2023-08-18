//! # Webserver
//! A simple webserver written in Rust.
//! This implements a thread pool and a webserver that can serve files and pages.
//! You can make a webserver that serves a page in just a few lines of code.
//! 
//! ## Example
//! ```
//! use simpleserve::server::{
//!    Webserver,
//!    Page,
//!    Sendable,
//!    HandlerFunction,
//!    RequestInfo
//! };
//! 
//! fn main() {
//!     let main_route: HandlerFunction = |_: &RequestInfo| -> Box<dyn Sendable + 'static> {
//!        Box::new(Page::new(200, String::from("Hello World!")))
//!     };
//!     let mut server = Webserver::new(10, vec![]);
//!     server.add_route("/", main_route);
//!     // server.start("127.0.0.1:7878");
//! }
//! ```

use std::{
    sync::{mpsc, Arc, Mutex},
    thread,
};

pub mod server;
pub mod utils;
pub mod errors;

pub use server::prelude::*;


/// A thread pool
/// 
/// This is used to execute functions/closures across multiple threads.
/// 
/// ## Example
/// ```
/// use simpleserve::ThreadPool;
/// 
/// let pool = ThreadPool::new(4);
/// 
/// for i in 0..20 {
///     pool.execute(move || {
///         println!("Job {}", i);
///     });
/// }
/// ```
pub struct ThreadPool {
    workers: Vec<Worker>,
    sender: Option<mpsc::Sender<Job>>,
}

type Job = Box<dyn FnOnce() + Send + 'static>;

impl ThreadPool {
    /// Create a new ThreadPool.
    ///
    /// The size is the number of threads in the pool.
    ///
    /// # Panics
    ///
    /// The `new` function will panic if the size is zero.
    pub fn new(size: usize) -> ThreadPool {
        assert!(size > 0);

        let (sender, receiver) = mpsc::channel();

        let receiver = Arc::new(Mutex::new(receiver));

        let mut workers = Vec::with_capacity(size);

        for _ in 0..size {
            workers.push(Worker::new(Arc::clone(&receiver)));
        }

        ThreadPool {
            workers,
            sender: Some(sender),
        }
    }

    /// Executes a closure.
    /// 
    /// Uses threads from pool to execute
    /// 
    /// # Panics
    /// 
    /// The `execute` function will fail if the job cannot be sent.
    pub fn execute<F>(&self, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let job = Box::new(f);

        self.sender
            .as_ref()
            .expect("Failed to get reference to sender")
            .send(job)
            .expect("Failed to send job");
    }

    pub fn stop(&mut self) {
        drop(self.sender.take());
        println!("Server stopped")
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        drop(self.sender.take());

        for worker in &mut self.workers {
            if let Some(thread) = worker.thread.take() {
                thread.join().unwrap();
            }
        }
    }
}

struct Worker {
    thread: Option<thread::JoinHandle<()>>,
}

impl Worker {
    fn new( receiver: Arc<Mutex<mpsc::Receiver<Job>>>) -> Worker {
        let thread = thread::spawn(move || loop {
            let message = receiver.lock().unwrap().recv();

            match message {
                Ok(job) => {
                    job();
                }
                Err(_) => {
                    break;
                }
            }
        });

        Worker {
            thread: Some(thread),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::server::Sendable;

    use super::*;
    use std::path;

    #[test]
    fn test_thread_pool() {
        let pool = ThreadPool::new(4);

        for i in 0..20 {
            pool.execute(move || {
                println!("Job {}", i);
            });
        }
    }

    #[test]
    fn test_thread_pool_drop() {
        let pool = ThreadPool::new(4);

        for i in 0..20 {
            pool.execute(move || {
                println!("Job {}", i);
            });
        }

        drop(pool);
    }

    #[test]
    fn test_server_routes() {
        let cargo_lock = path::Path::new("Cargo.lock").canonicalize().unwrap();
        let handlers: server::HandlerFunction = |_| -> Box<dyn Sendable + 'static> {
            Box::new(server::Page::new(200, String::from("Hello World!")))
        };
        let mut server = server::Webserver::new(10, vec![cargo_lock.clone()]);
        server.add_route("/", handlers.clone());
        server.add_route("/sleep", handlers.clone());
        server.add_route("/image.jpg", handlers.clone());
        server.add_accessible_files(vec!["src/lib.rs", "src/server.rs"]).unwrap();
        assert_eq!(server.blacklisted_paths()[0], cargo_lock);
    }
}