use std::task::Poll;

use bytes::Bytes;
use futures::stream;
use any_spawner::Executor;
use leptos::{config::get_configuration, error::Error};
use leptos_wasi::prelude::{Body, WasiExecutor};
use wasi::{
    exports::http::incoming_handler::Guest, 
    filesystem::{preopens::get_directories, types::{DescriptorFlags, OpenFlags, PathFlags}}, 
    http::{
        proxy::export,
        types::{IncomingRequest, ResponseOutparam},
    }
};

use crate::routes::{shell, App};

struct LeptosServer;

impl Guest for LeptosServer {
    fn handle(request: IncomingRequest, response_out: ResponseOutparam) {
        // Initiate a single-threaded Future Executor so we can run the
        // rendering system and take advantage of bodies streaming.
        let executor = WasiExecutor::new(leptos_wasi::executor::Mode::Stalled);
        Executor::init_local_custom_executor(executor.clone())
            .expect("cannot init future executor");
        executor.run_until(async {
            handle_request(request, response_out).await;
        })
    }
}

async fn handle_request(request: IncomingRequest, response_out: ResponseOutparam) {
    use leptos_wasi::prelude::Handler;
    // Import your server functions here
    use crate::pages::home::UpdateCount;
    use crate::pages::home::GetCount;

    let conf = get_configuration(None).unwrap();
    let leptos_options = conf.leptos_options;

    // Debug: Log the incoming request
    println!("Incoming request: {:?}", request.path_with_query());
    
    Handler::build(request, response_out)
        .expect("could not create handler")
        // All static assets should be served on /pkg/...
        // when the user request this path, the passed function is called
        .static_files_handler("/pkg", serve_static_files)
        // Register your server functions here
        .with_server_fn_axum::<UpdateCount>()
        .with_server_fn_axum::<GetCount>()
        // Fetch all available routes from your App.
        .generate_routes(App)
        // Actually process the request and write the response.
        .handle_with_context(move || shell(leptos_options.clone()), || {})
        .await
        .expect("could not handle the request");
}

fn serve_static_files(path: String) -> Option<Body> {
    println!("serving static file: {}", &path);
    let directories = get_directories();

    // Debug: Print available directories
    println!("Available directories: {:?}", directories.len());
    for (i, (_fd, mount_path)) in directories.iter().enumerate() {
        println!("Directory {}: {}", i, mount_path);
    }

    // The path comes in as /counter.css, /counter.js, etc.
    // Strip the leading slash
    let path = path.strip_prefix("/").unwrap_or(&path);

    // Check runtime and adjust path accordingly
    #[cfg(runtime_spin)]
    let file_path = format!("pkg/{}", path); // Spin needs pkg/ prefix

    #[cfg(not(runtime_spin))]
    let file_path = path.to_string(); // Wasmtime mounts directly at root

    println!("Looking for file at: {}", file_path);

    let (fd, _) = directories.first().expect("there seems to be no static files to serve");

    match fd.open_at(PathFlags::empty(), &file_path, OpenFlags::empty(), DescriptorFlags::READ) {
        Err(err) => {
            println!("could not serve file {}", file_path);
            println!("reason: {}", err.message());
            return None;
        },
        Ok(fd) => {
            let file_size = fd.stat().expect("should be able to stat").size;
            match fd.read_via_stream(0) {
                Err(err) => {
                    println!("could not open stream to file {}", file_path);
                    println!("reason: {}", err.message());
                    return None;
                },
                Ok(stream) => {
                    let mut read_bytes: u64 = 0;
                    return Some(
                        Body::Async(
                            Box::pin(stream::poll_fn(move |_| -> Poll<Option<Result<Bytes, Error>>> {
                                if read_bytes >= file_size {
                                    return Poll::Ready(None)
                                }

                                match stream.blocking_read(256) {
                                    Err(err) => Poll::Ready(Some(Err(err.into()))),
                                    Ok(data) => {
                                        read_bytes += data.len() as u64;
                                        return Poll::Ready(Some(Ok(Bytes::from(data))));
                                    }
                                }
                            }))
                        )
                    );
                }
            }
        }
    }
}

// CRITICAL: Export the server as a WASI component
export!(LeptosServer with_types_in wasi);