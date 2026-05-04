# Leptos Wasi Demo Using `wasmtime` and `spin_sdk` `v5`

This project supports both **Spin** and **Wasmtime** runtimes without using feature flags. The runtime is selected at build time using environment variables.

## Usage

### Option 1: Using Spin Runtime
```bash
make spin
```
- Builds with Spin's key-value store support
- Runs on http://127.0.0.1:3000
- Data stored in `.spin/default.db`
- Uses spin-sdk for KV storage

### Option 2: Using Wasmtime Runtime
```bash
make wasmtime
```
- Builds with filesystem storage
- Runs on http://127.0.0.1:3000
- Data stored in `data/` directory
- Uses standard filesystem APIs


## Prerequisites

### 1. Install Rust with WASI support
```bash
# Install Rust if not already installed
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Add WASI target
rustup target add wasm32-wasip2
```

### 2. Install Spin (for Spin runtime)
```bash
# On macOS
brew install fermyon/tap/spin

# Or using installer script
curl -fsSL https://developer.fermyon.com/downloads/install.sh | bash

# Verify installation
spin --version
```

### 3. Install Wasmtime (for Wasmtime runtime)
```bash
# On macOS
brew install wasmtime

# Or download from releases
curl -L https://github.com/bytecodealliance/wasmtime/releases/download/v24.0.0/wasmtime-v24.0.0-x86_64-macos.tar.xz | tar xJ
sudo mv wasmtime-v24.0.0-x86_64-macos/wasmtime /usr/local/bin/

# Verify installation (need v14.0+ for serve command)
wasmtime --version
```

### 4. Install cargo-leptos
```bash
cargo install cargo-leptos
```

### 5. Install Make (usually pre-installed)
```bash
# On macOS
brew install make

# Verify
make --version
```

## How It Works

### Architecture
```
┌─────────────────┐
│  User Request   │
└────────┬────────┘
         │
    ┌────▼────┐
    │ Makefile │
    └────┬────┘
         │
    ┌────▼────┐
    │ build.rs │ ← Detects WASI_RUNTIME env var
    └────┬────┘
         │
    ┌────▼────────────────┐
    │ Conditional Compile  │
    └──┬─────────────┬────┘
       │             │
  ┌────▼───┐    ┌───▼──────┐
  │  Spin  │    │ Wasmtime │
  │  Build │    │  Build   │
  └────┬───┘    └───┬──────┘
       │            │
  ┌────▼───┐    ┌───▼──────┐
  │Spin KV │    │Filesystem│
  │Storage │    │ Storage  │
  └────────┘    └──────────┘
```


## Project Structure
```
counter/
├── Makefile           # Build orchestration
├── build.rs           # Runtime detection
├── Cargo.toml         # Dependencies
├── spin.toml          # Spin configuration
├── src/
│   ├── lib.rs         # Main library
│   ├── storage.rs     # Runtime-agnostic storage
│   └── pages/
│       └── home.rs    # Counter logic
├── target/
│   ├── wasm32-wasip2/ # Spin build output
│   └── wasmtime/      # Wasmtime file storage
│   └── site/
│       └── data       # Data Store Wasmtime
└── .spin/             # Spin KV storage
```

## How Runtime Selection Works

1. **Environment Variable**: `WASI_RUNTIME` is set to either `spin` or `wasmtime`
2. **build.rs**: Reads this variable and sets compile-time flags
3. **Conditional Compilation**: Code in `storage.rs` compiles differently based on flags
4. **No Feature Flags**: Everything controlled by environment variables

## Storage Backends

| Runtime  | Storage Type | Location | Technology |
|----------|-------------|----------|------------|
| Spin     | Key-Value   | `.spin/default.db` | spin-sdk KV store |
| Wasmtime | Filesystem  | `data/` directory | Standard file I/O |


## License

This project is licensed under the MIT License - see the LICENSE file for details.