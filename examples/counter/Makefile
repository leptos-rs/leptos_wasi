# Makefile for building counter app for different WASI runtimes
.PHONY: help spin wasmtime clean run

# Default target
help:
	@echo "Available targets:"
	@echo "  make spin     - Build and run with Spin"
	@echo "  make wasmtime - Build and run with Wasmtime"
	@echo "  make clean    - Clean build artifacts"

# Run target alias
run: wasmtime

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
	WASI_RUNTIME=wasmtime cargo leptos build --release
	WASI_RUNTIME=wasmtime LEPTOS_OUTPUT_NAME=counter cargo build --lib --target wasm32-wasip2 --release --no-default-features --features ssr --target-dir target/wasmtime
	@echo "Built: target/wasmtime/wasm32-wasip2/release/counter.wasm (for Wasmtime)"
	@echo "Starting Wasmtime serve..."
	@echo "Serving at http://127.0.0.1:3000"
	@mkdir -p ./data
	wasmtime serve \
		-W component-model-async=y \
		-S p3=y \
		-S cli=y \
		-S http=y \
		--dir=./target/site/pkg::/ \
		--dir=./data::/data \
		--env=LEPTOS_OUTPUT_NAME=counter \
		--env=LEPTOS_SITE_ROOT=/ \
		--env=LEPTOS_SITE_PKG_DIR=pkg \
		--addr 127.0.0.1:3000 \
		target/wasmtime/wasm32-wasip2/release/counter.wasm

# Clean target
clean:
	cargo clean
	rm -rf target/site target/wasmtime ./data
	rm -rf .spin
