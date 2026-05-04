// Main function is required even though it's unused on WASI targets
fn main() {
    #[cfg(feature = "ssr")]
    {
        // Unused since there is no "main" function on WASI targets
        // but the build system would complain otherwise.
    }
}