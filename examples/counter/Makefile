# Makefile for building counter app for different WASI runtimes

.PHONY: help spin wasmtime clean

# Default target
help:
	@echo "Available targets:"
	@echo "  make spin     - Build and run with Spin"
	@echo "  make wasmtime - Build and run with Wasmtime"
	@echo "  make clean    - Clean build artifacts"

# Build and run with Spin
spin:
	@echo "Building for Spin runtime..."
	@mkdir -p .spin
	WASI_RUNTIME=spin cargo leptos build --release
	WASI_RUNTIME=spin LEPTOS_OUTPUT_NAME=counter cargo build --lib --target wasm32-wasip2 --release --no-default-features --features ssr
	@echo "Built: target/wasm32-wasip2/release/counter.wasm (for Spin)"
	@echo "Starting Spin..."
	spin up

# Build and run with Wasmtime
wasmtime:
	@echo "Building for Wasmtime runtime..."
	@mkdir -p components
	@if [ ! -f "components/spin_static_fs.wasm" ]; then \
		echo "Downloading fileserver component..."; \
		curl -L https://github.com/fermyon/spin-fileserver/releases/download/v0.1.0/spin_static_fs.wasm \
			-o components/spin_static_fs.wasm; \
	fi
	WASI_RUNTIME=wasmtime cargo leptos build --release
	WASI_RUNTIME=wasmtime LEPTOS_OUTPUT_NAME=counter cargo build --lib --target wasm32-wasip2 --release --no-default-features --features ssr --target-dir target/wasmtime
	@echo "Built: target/wasmtime/wasm32-wasip2/release/counter.wasm (for Wasmtime)"
	@echo "Starting Wasmtime HTTP server..."
	@echo "Static files served from: target/site/pkg/"
	@echo "Access at: http://127.0.0.1:3000"
	wasmtime serve \
		-S cli=y \
		-S http=y \
		--dir=./target/site/pkg::/ \
		--env=LEPTOS_OUTPUT_NAME=counter \
		--env=LEPTOS_SITE_ROOT=/ \
		--env=LEPTOS_SITE_PKG_DIR=pkg \
		--addr 127.0.0.1:3000 \
		target/wasmtime/wasm32-wasip2/release/counter.wasm

# Clean build artifacts
clean:
	cargo clean
	rm -rf target/site target/wasmtime
	rm -rf .spin
	rm -rf components