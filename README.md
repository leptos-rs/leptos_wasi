<div align="center">
  <h1><code>leptos_wasi</code></h1>

  <p>
    <strong>Run your Leptos Server-Side in
    <a href="https://webassembly.org/">WebAssembly</a>
    using WASI standards.
    </strong>
  </p>
</div>

## Explainer

WebAssembly is already popular in the browser but organisations like the
[Bytecode Alliance][bc-a] are committed to providing the industry with new
standard-driven ways of running software. Specifically, they are maintaining
the [Wasmtime][wasmtime] runtime, which allows running WebAssembly out of the
browser (e.g., on a serverless platform).

Leptos is already leveraging WebAssembly in the browser and gives you tools to
build web applications with best-in-class performance.

This crate aims to go further and enable you to also leverage WebAssembly for
your [Leptos Server][leptos-server]. Specifically, it will allow you to
target the rust `wasm32-wasip2` target for the server-side while integrating
seamlessly with the Leptos Framework.

Running `cargo leptos build` will provide you with a
[WebAssembly Component][wasm-component] importing the
[`wasi:http/proxy` world][wasi-http-proxy] (or WASIp3 equivalents). This means you can serve
your server on any runtime supporting this world, for example:

For WASI Preview 2:
```shell
wasmtime serve target/server/wasm32-wasip2/debug/your_crate.wasm -Scommon
```

For WASI Preview 3:
```shell
wasmtime serve -W component-model-async=y -S p3=y target/server/wasm32-wasip2/debug/your_crate.wasm
```

[bc-a]: https://bytecodealliance.org/
[leptos-server]: https://book.leptos.dev/server/index.html
[wasmtime]: https://wasmtime.dev
[wasi-http-proxy]: https://github.com/WebAssembly/wasi-http/blob/main/proxy.md
[wasm-component]: https://component-model.bytecodealliance.org

## Disclaimer

This crate is **EXPERIMENTAL** and the author is not affiliated with the Bytecode
Alliance nor funded by any organisation. Consider this crate should become a
community-driven project and be battle-tested to be deemed *production-ready*.

Contributions are welcome!

## What's New in v0.3.0

- **Native WASI Preview 3 support** via the `wasi-p3` feature flag — task spawning is delegated directly to the host runtime using `wasip3::wit_bindgen::spawn`, eliminating guest-side cooperative polling overhead.
- **`init_wasip3_spawner()` API** for initializing the WASIp3 executor at your server entrypoint.
- **Async `Handler::build()`** under WASIp3 (different signature from WASIp2).
- **Spin SDK v6 / Spin v4** compatibility with native WASIp3 HTTP triggers.
- **Dual-runtime counter example** (`examples/counter`) supporting both raw Wasmtime and Spin via compile-time `build.rs` runtime detection.
- **Spin-only counter example** (`examples/spin-counter`) using Spin's built-in key-value store.
- **Upgraded Leptos ecosystem:** `leptos` 0.8.9, `server_fn` 0.8.7, `leptos_router` 0.8.7, `leptos_meta` 0.8.5, `leptos_macro` 0.8.8.
- **New dependency:** `wasip3 = "0.6.0"` (WASIp3 core types, host spawner bindings, HTTP compatibility layers).
- **New dependency:** `http-body = "1.0.0"` (standard streaming response frames for the WASIp3 pipeline).

## Prerequisites

To compile and run applications utilizing this crate, the following tools are required:
- **Rust Toolchain:** Version 1.85.0 or later (required for edition 2024).
- **Rust Target:** `wasm32-wasip2` (run `rustup target add wasm32-wasip2`).
- **Cargo Leptos:** `cargo install --locked cargo-leptos`.
- **Wasmtime CLI:** Version 45.0.0 or later (if serving under Wasmtime).
- **Spin CLI:** Version 4.0.0 or later (if serving under Spin).

## Compatibility

- **Leptos:** `0.8.9` fully tested.
- **Spin SDK:** `v6.0.0` (WASIp3) fully tested.
- **WASI Bindings:**
  - `wasi = "0.13.1"` — WASIp2 types and polling interfaces.
  - `wasip3 = "0.6.0"` — WASIp3 core types, host spawner, and HTTP compatibility layers.
- **WASI Features:**
  - `wasi-p2` (Default): Built-in cooperative async polling executor.
  - `wasi-p3`: Native host-level task spawning utilizing `wasip3::wit_bindgen::spawn`.

## Feature Flags

Add the following to your `Cargo.toml`:

```toml
[dependencies]
# For WASI Preview 2 (default):
leptos_wasi = "0.3.0"

# For WASI Preview 3:
leptos_wasi = { version = "0.3.0", default-features = false, features = ["wasi-p3"] }
```

> [!NOTE]
> Compiling with both features enabled simultaneously (e.g. `--all-features` in workspaces) is fully supported. When both `wasi-p2` and `wasi-p3` flags are active, the `wasi-p3` pipeline and native host spawning take precedence automatically.

## Usage

### 1. WASI Preview 3 (WASIp3) Mode

Under WASIp3, task spawning is handled natively by the host runtime, so you do not need to run a guest-side cooperative polling loop. Simply register the spawner at the server entrypoint.

#### Server Entrypoint (`src/server.rs`):
```rust
use leptos::config::get_configuration;
use leptos_wasi::executor::init_wasip3_spawner;
use leptos_wasi::prelude::Handler;
use wasip3::http::types::{Request, Response, ErrorCode};

use crate::app::{shell, App, GetCount};

struct LeptosServer;

impl wasip3::exports::http::handler::Guest for LeptosServer {
    async fn handle(request: Request) -> Result<Response, ErrorCode> {
        // 1. Initialize host async task scheduling
        let _ = init_wasip3_spawner();

        let conf = get_configuration(None).unwrap();
        let leptos_options = conf.leptos_options;

        // Convert the WASI request to http::Request
        let req = wasip3::http_compat::http_from_wasi_request(request)?;

        // 2. Build and handle request natively
        let wasi_res = Handler::build(req).await
            .map_err(|_| ErrorCode::InternalError(None))?
            .with_server_fn_axum::<GetCount>()
            .generate_routes(App)
            .handle_with_context(move || shell(leptos_options.clone()), || {})
            .await
            .map_err(|_| ErrorCode::InternalError(None))?;

        Ok(wasi_res)
    }
}

wasip3::http::service::export!(LeptosServer);
```

### 2. WASI Preview 2 (WASIp2) Mode

Under WASIp2, you must instantiate the custom `WasiExecutor` and block the guest execution thread on the cooperative polling loop to drive async futures.

```rust
use any_spawner::Executor as LeptosExecutor;
use leptos_wasi::prelude::{IncomingRequest, ResponseOutparam, WasiExecutor};
use wasi::exports::http::incoming_handler::Guest;

struct LeptosServer;

impl Guest for LeptosServer {
    fn handle(request: IncomingRequest, response_out: ResponseOutparam) {
        // Initialize guest-side polling executor
        let executor = WasiExecutor::new(leptos_wasi::executor::Mode::Stalled);
        LeptosExecutor::init_local_custom_executor(executor.clone()).unwrap();

        executor.run_until(async {
            Handler::build(request, response_out).unwrap()
                .with_server_fn_axum::<GetCount>()
                .generate_routes(App)
                .handle_with_context(move || shell(leptos_options.clone()), || {})
                .await
                .unwrap();
        })
    }
}

wasi::http::proxy::export!(LeptosServer with_types_in wasi);
```

### Server Function Registration

This crate provides multiple convenient ways to register server functions:

#### 🎯 **Recommended: Axum Backend (Most Common)**
```rust
.with_server_fn_axum::<MyServerFn>()
```
Perfect for most Leptos projects using the default axum backend.

#### 🔧 **Generic Backend**
```rust
.with_server_fn_generic::<MyServerFn>()
```
For projects using custom server backends.

#### 🛠️ **Advanced: Explicit Control**
```rust
.with_server_fn::<MyServerFn, BodyType>()
```
When you need full control over body types.

### Static File Serving

```rust
fn serve_static_files(path: String) -> Option<leptos_wasi::response::Body> {
    // Your static file serving logic
    // Return None for 404, Some(body) for file content
}

handler.static_files_handler("/public", serve_static_files)
```

## Migration Guide

### Upgrading to v0.2.0+

If you're using the older syntax with type placeholders, you can easily upgrade:

#### Before (v0.1.3 and earlier)

```rust
.with_server_fn::<GetCount>()
```

#### After (v0.2.0+)
```rust
.with_server_fn::<GetCount,_>() // for custom backend ResponseBody
.with_server_fn_axum::<UpdateCount>() 
.with_server_fn_generic::<GetCount>()
```

### Static File Handler Updates

The static file handler now expects `leptos_wasi::response::Body` directly:

```rust
// Updated signature
fn serve_static_files(path: String) -> Option<leptos_wasi::response::Body> {
    // Implementation remains the same
}
```

## Examples

Check out the following sample applications in this repository:
- **[counter](./examples/counter)**: A complete Leptos counter application running under native WASI Preview 3, supporting both raw Wasmtime and Spin as runtimes.
- **[spin-counter](./examples/spin-counter)**: A Leptos counter application compiled specifically for the Spin SDK using the Spin Key-Value store.

## Core Features

* :octopus: **Async Runtime**: This crate comes with a single-threaded *async* executor
  making full use of WASIp2 [`pollable`][wasip2-pollable], so your server is not
  blocking on I/O and can benefit from Leptos' streaming [SSR Modes][leptos-ssr-modes].
* :zap: **Short-circuiting Mechanism**: Your component is smart enough to avoid
  preparing or doing any *rendering* work if the request routes to static files or
  *Server Functions*.
* :truck: **Custom Static Assets Serving**: You can write your own logic
  for serving static assets. For example, once
  [`wasi:blobstore`][wasi-blobstore] matures up, you could host your static assets
  on your favorite *Object Storage* provider and make your server fetch them
  seamlessly.
* :gear: **Multiple Server Backends**: Works seamlessly with axum, generic, and custom server function backends.

## Troubleshooting

### Common Issues

#### Server function not found (404)
Ensure your server function is properly registered:
```rust
.with_server_fn_axum::<YourServerFn>()  // Must match your #[server] macro
```

### Build Commands

```bash
# Build for WASI target
cargo build --target wasm32-wasip2

# Run under WASI Preview 2:
wasmtime serve target/wasm32-wasip2/release/your_crate.wasm -Scommon

# Run under WASI Preview 3:
wasmtime serve -W component-model-async=y -S p3=y target/wasm32-wasip2/release/your_crate.wasm

# For leptos projects
cargo leptos build --release
```

[leptos-ssr-modes]: https://book.leptos.dev/ssr/23_ssr_modes.html
[wasip2-pollable]: https://github.com/WebAssembly/wasi-io/blob/main/wit/poll.wit
[wasi-blobstore]: https://github.com/WebAssembly/wasi-blobstore
