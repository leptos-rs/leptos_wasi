# spin-counter

This example demonstrates running a Leptos application utilizing the native WASI Preview 3 (WASIp3) task scheduling and standard HTTP wrappers under Spin v4.

## Prerequisites

- **Rust Toolchain:** Version 1.93.0 or later (required by spin-sdk).
- **Rust target:** `rustup target add wasm32-wasip2`
- **Spin CLI:** Version 4.0.0 or later.
- **Cargo Leptos:** `cargo install --locked cargo-leptos`

## Build and Run

```bash
spin build --up
```

Once running, access the application at `http://127.0.0.1:3000`.

## Execution Model

Unlike the WASIp2 approach which manages a cooperative polling loop inside the guest component:
1. `spin-counter` initializes the host-level async task spawner (`init_wasip3_spawner`).
2. When futures are spawned (e.g. via `leptos::spawn_local`), they are delegated directly to the host runtime via `wasip3::wit_bindgen::spawn`.
3. The HTTP trigger uses the native `http` executor in `spin.toml`, running the server as a native WASI HTTP component.
