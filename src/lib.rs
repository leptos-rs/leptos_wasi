//! A first-party support of the `wasm32-wasip1` target for the **Server-Side**
//! of Leptos using the [`wasi:http`][wasi-http] proposal.
//!
//! [wasi-http]: https://github.com/WebAssembly/wasi-http
//!
//! # `Handler`
//!
//! The [`prelude::Handler`] is the main abstraction you will use.
//!
//! It expects being run in the context of a Future Executor `Task`,
//! since WASI is, at the moment, a single-threaded environment,
//! we provide a simple abstraction in the form of [`any_spawner::Executor`]
//! that you can leverage to use this crate.
//!
//! ```
//! use any_spawner::Executor;
//! use leptos_wasi::prelude::WasiExecutor;
//! use wasi::exports::http::incoming_handler::*;
//!
//! struct LeptosServer;
//!
//! impl Guest for LeptosServer {
//!     fn handle(request: IncomingRequest, response_out: ResponseOutparam) {
//!         // Initiate a single-threaded [`Future`] Executor so we can run the
//!         // rendering system and take advantage of bodies streaming.
//!         let executor =
//!             WasiExecutor::new(leptos_wasi::executor::Mode::Stalled);
//!         Executor::init_local_custom_executor(executor.clone())
//!             .expect("cannot init future executor");
//!         executor.run_until(async {
//!             //handle_request(request, response_out).await;
//!         })
//!     }
//! }
//! ```
//!
//! # WASI Bindings
//!
//! We are using the bindings provided by the `wasi` crate.

pub mod executor;
pub mod handler;
pub mod request;
pub mod response;
pub mod utils;

#[allow(clippy::pub_use)]
pub mod prelude {
    pub use crate::{
        executor::Executor as WasiExecutor, handler::Handler, response::Body,
        utils::redirect,
    };
    pub use http::StatusCode;
    pub use wasi::exports::wasi::http::incoming_handler::{
        IncomingRequest, ResponseOutparam,
    };
}

/// When working with streams, this crate will try to chunk bytes with
/// this size.
const CHUNK_BYTE_SIZE: usize = 64;
