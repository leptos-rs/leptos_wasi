# counter

This example demonstrates running a Leptos application utilizing the native WASI Preview 3 (WASIp3) task scheduling and standard HTTP triggers. It supports both raw Wasmtime and Spin as runtimes.

## Prerequisites

- **Rust Toolchain:** Version 1.93.0 or later (required by spin-sdk).
- **Rust target:** `rustup target add wasm32-wasip2`
- **Cargo Leptos:** `cargo install --locked cargo-leptos`
- **Spin CLI:** Version 4.0.0 or later.
- **Wasmtime CLI:** Version 45.0.0 or later.

## Build and Run

To compile and run the application under Wasmtime:

```bash
make wasmtime
```

To compile and run the application under Spin:

```bash
make spin
```

To clean up all local build and storage files:

```bash
make clean
```

Once running, access the application at `http://127.0.0.1:3000`.

## Architecture & Storage Backend

1. **Storage:** The storage mechanism depends on the runtime:
   - **Spin:** Persists the count using Spin's built-in key-value store (configured as the `"default"` store in `spin.toml`).
   - **Wasmtime:** Persists the count to `/data/counter.txt` inside the component's sandboxed filesystem, mapped via `--dir=./data::/data` to a local directory on your host.
2. **Static Files:** `./target/site/pkg/` is mapped by Wasmtime to `/` inside the guest for static file serving.
3. **WASI HTTP:** The server implements `wasip3::exports::http::handler::Guest` and runs as a native WebAssembly component using the Preview 3 async ABI.
