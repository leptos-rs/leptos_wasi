use std::env;

fn main() {
    // Tell cargo to check our custom cfg flags
    println!("cargo::rustc-check-cfg=cfg(runtime_spin)");
    println!("cargo::rustc-check-cfg=cfg(runtime_wasmtime)");

    // Check for WASI_RUNTIME environment variable
    let runtime = env::var("WASI_RUNTIME").unwrap_or_else(|_| "wasmtime".to_string());

    println!("cargo:rustc-env=WASI_RUNTIME={}", runtime);

    // Set compile-time flags based on runtime
    match runtime.as_str() {
        "spin" => {
            println!("cargo:rustc-cfg=runtime_spin");
            println!("cargo:warning=Building for Spin runtime");
        }
        "wasmtime" => {
            println!("cargo:rustc-cfg=runtime_wasmtime");
            println!("cargo:warning=Building for Wasmtime runtime");
        }
        _ => {
            println!("cargo:rustc-cfg=runtime_wasmtime");
            println!("cargo:warning=Unknown runtime '{}', defaulting to Wasmtime", runtime);
        }
    }

    // You can also check for specific environment markers
    if env::var("SPIN_BUILD").is_ok() {
        println!("cargo:rustc-cfg=runtime_spin");
        println!("cargo:warning=SPIN_BUILD detected, building for Spin");
    }
}