# spin-counter-p3

This example demonstrates running a Leptos application utilizing the native WASI Preview 3 (WASIp3) task scheduling and standard HTTP wrappers under Spin v4.

## Prerequisites

- **Rust target:** `rustup target add wasm32-wasip2` (the standard target for WASI Preview 2/3 component compilation).
- **Spin CLI:** **Spin v4.0.0** (or later) is required. A compatible binary is provided in the repository under `./bin/spin`.
- **Cargo Leptos:** `cargo install --locked cargo-leptos` (used to compile the assets and component bindings).

## Build and Run

To build all assets, compile the component, and start the server, run:

```bash
../../bin/spin build --up
```

Once running, you can access the application at `http://127.0.0.1:3000`.

## Rationale and Execution Model

Unlike the WASIp2 example which manages a cooperative polling loop inside the guest component:
1. `spin-counter-p3` initializes the host-level async task spawner (`init_wasip3_spawner`).
2. When futures are spawned (e.g. via `leptos::spawn_local`), they are delegated directly to the host runtime via `wasip3::wit_bindgen::spawn`.
3. The HTTP trigger uses the native `http` executor in `spin.toml`, running the server as a native WASI HTTP component.
