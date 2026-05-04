use server_fn::ServerFnError;

// Direct implementations without dynamic dispatch since async traits aren't dyn compatible

#[cfg(runtime_spin)]
pub async fn get(key: &str) -> Result<Option<Vec<u8>>, ServerFnError> {
    use spin_sdk::key_value::Store;

    let store = Store::open_default()
        .map_err(|e| ServerFnError::new(format!("Failed to open Spin KV store: {}", e)))?;

    store.get(key)
        .map_err(|e| ServerFnError::new(format!("Failed to get from Spin KV: {}", e)))
}

#[cfg(runtime_spin)]
pub async fn set(key: &str, value: &[u8]) -> Result<(), ServerFnError> {
    use spin_sdk::key_value::Store;

    let store = Store::open_default()
        .map_err(|e| ServerFnError::new(format!("Failed to open Spin KV store: {}", e)))?;

    store.set(key, value)
        .map_err(|e| ServerFnError::new(format!("Failed to set in Spin KV: {}", e)))
}

#[cfg(runtime_wasmtime)]
pub async fn get(key: &str) -> Result<Option<Vec<u8>>, ServerFnError> {
    use std::fs;
    use std::path::Path;

    let base_path = std::env::var("STORAGE_PATH").unwrap_or_else(|_| "./data".to_string());
    let file_path = format!("{}/{}.txt", base_path, key);
    let path = Path::new(&file_path);

    if !path.exists() {
        return Ok(None);
    }

    fs::read(&file_path)
        .map(Some)
        .map_err(|e| ServerFnError::new(format!("Failed to read file: {}", e)))
}

#[cfg(runtime_wasmtime)]
pub async fn set(key: &str, value: &[u8]) -> Result<(), ServerFnError> {
    use std::fs;
    use std::path::Path;

    let base_path = std::env::var("STORAGE_PATH").unwrap_or_else(|_| "./data".to_string());

    // Ensure directory exists
    let dir_path = Path::new(&base_path);
    if !dir_path.exists() {
        fs::create_dir_all(dir_path)
            .map_err(|e| ServerFnError::new(format!("Failed to create directory: {}", e)))?;
    }

    let file_path = format!("{}/{}.txt", base_path, key);
    fs::write(&file_path, value)
        .map_err(|e| ServerFnError::new(format!("Failed to write file: {}", e)))
}

// Fallback implementations for when neither runtime is specified
#[cfg(not(any(runtime_spin, runtime_wasmtime)))]
pub async fn get(key: &str) -> Result<Option<Vec<u8>>, ServerFnError> {
    use std::fs;
    use std::path::Path;

    let base_path = "./data";
    let file_path = format!("{}/{}.txt", base_path, key);
    let path = Path::new(&file_path);

    if !path.exists() {
        return Ok(None);
    }

    fs::read(&file_path)
        .map(Some)
        .map_err(|e| ServerFnError::new(format!("Failed to read file: {}", e)))
}

#[cfg(not(any(runtime_spin, runtime_wasmtime)))]
pub async fn set(key: &str, value: &[u8]) -> Result<(), ServerFnError> {
    use std::fs;
    use std::path::Path;

    let base_path = "./data";

    // Ensure directory exists
    let dir_path = Path::new(&base_path);
    if !dir_path.exists() {
        fs::create_dir_all(dir_path)
            .map_err(|e| ServerFnError::new(format!("Failed to create directory: {}", e)))?;
    }

    let file_path = format!("{}/{}.txt", base_path, key);
    fs::write(&file_path, value)
        .map_err(|e| ServerFnError::new(format!("Failed to write file: {}", e)))
}