# counter-p3

This example demonstrates running a Leptos application utilizing the native WASI Preview 3 (WASIp3) task scheduling and standard HTTP triggers under raw Wasmtime without using Spin.

## Prerequisites

- **Wasmtime CLI:** `wasmtime` version 14.0 or later.
- **Rust target:** `rustup target add wasm32-wasip2`
- **Cargo Leptos:** `cargo install --locked cargo-leptos`
- **wasm-bindgen-cli Mismatch Workaround:**
  If your system's global `wasm-bindgen-cli` (or cargo-leptos) version does not match the crate dependency, run:
  ```bash
  cargo update -p wasm-bindgen --precise 0.2.103
  ```
  (Or whatever version matches your local CLI)

## Build and Run

To compile and run the application under Wasmtime using the local binary (Wasmtime v43.0.1+):

```bash
make wasmtime
```

To compile and run the application under Spin using the local binary (Spin v4.0.0+):

```bash
make spin
```

To clean up all local build and storage files:

```bash
make clean
```

Once running, access the application at `http://127.0.0.1:3000`.

## Local Binary Prerequisites

The `Makefile` targets execute local binaries located in the `./bin/` directory at the project root (`../../bin/wasmtime` and `../../bin/spin`). If the binaries are not present, populate them by running the curl commands documented in the root [README](../../README.md#setting-up-local-binaries).

## Architecture & Storage Backend

1. **Storage:** Persists the count to `/data/counter.txt` inside the component's sandboxed filesystem. This is mapped via `--dir=./data::/data` by Wasmtime to a local directory on your host.
2. **Preopened FS:** Static files inside `./target/site/pkg/` are mapped by Wasmtime to `/` inside the guest, which the static files handler reads directly.
3. **WASI HTTP World:** The server implements `wasip3::exports::http::handler::Guest` and is compiled into a raw WebAssembly component. Wasmtime executes it natively using the standard Preview 3 async ABI.
