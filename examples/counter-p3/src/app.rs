use components::{Route, Router, Routes};
use leptos::prelude::*;
use leptos_meta::*;
use leptos_router::*;

#[cfg(feature = "hydrate")]
use web_sys::window;

#[cfg(feature = "ssr")]
pub fn shell(options: LeptosOptions) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1" />
                <AutoReload options=options.clone() />
                <HydrationScripts options=options.clone() root="" />
                <MetaTags />
            </head>
            <body>
                <App />
            </body>
        </html>
    }
}

#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();

    let fallback = || view! { "Page not found." }.into_view();

    view! {
        <Stylesheet id="leptos" href="/pkg/counter_p3.css" />
        <Meta
            name="description"
            content="A website running its server-side as a WASIp3 Component :D"
        />

        <Title text="Welcome to Leptos X WASIp3!" />

        <Router>
            <main>
                <Routes fallback>
                    <Route path=path!("") view=HomePage />
                    <Route path=path!("/*any") view=NotFound />
                </Routes>
            </main>
        </Router>
    }
}

#[component]
fn HomePage() -> impl IntoView {
    let increment_action = ServerAction::<IncrementCount>::new();
    let (optimistic_count, set_optimistic_count) = signal(None::<u32>);
    let count = Resource::new(move || increment_action.version().get(), |_| get_count());

    Effect::new(move |_| {
        if optimistic_count.get().is_none() {
            #[cfg(feature = "hydrate")]
            {
                if let Some(window) = window() {
                    if let Ok(Some(storage)) = window.local_storage() {
                        if let Ok(Some(cached_count_str)) = storage.get_item("counter_count") {
                            if let Ok(cached_count) = cached_count_str.parse::<u32>() {
                                set_optimistic_count.set(Some(cached_count));
                                return;
                            }
                        }
                    }
                }
            }

            if let Some(Ok(server_count)) = count.get() {
                set_optimistic_count.set(Some(server_count));

                #[cfg(feature = "hydrate")]
                {
                    if let Some(window) = window() {
                        if let Ok(Some(storage)) = window.local_storage() {
                            let _ = storage.set_item("counter_count", &server_count.to_string());
                        }
                    }
                }
            }
        }
    });

    Effect::new(move |_| {
        if let Some(Ok(server_count)) = count.get() {
            if let Some(current_optimistic) = optimistic_count.get() {
                if server_count != current_optimistic {
                    set_optimistic_count.set(Some(server_count));
                }
            }

            #[cfg(feature = "hydrate")]
            {
                if let Some(window) = window() {
                    if let Ok(Some(storage)) = window.local_storage() {
                        let _ = storage.set_item("counter_count", &server_count.to_string());
                    }
                }
            }
        }
    });

    let display_count = move || {
        if let Some(opt_count) = optimistic_count.get() {
            opt_count.to_string()
        } else {
            "...".to_string()
        }
    };

    view! {
        <div class="min-h-screen bg-[#1a2332] flex items-center justify-center p-4">
            <div class="bg-[#263343] rounded-xl shadow-2xl p-8 md:p-12 max-w-md w-full border border-[#3a4a5c]">
                <div class="text-center space-y-8">
                    <div class="space-y-2">
                        <div class="flex items-center justify-center gap-3 mb-4">
                            <div class="w-10 h-10 bg-[#00d4aa] rounded-lg flex items-center justify-center">
                                <span class="text-[#1a2332] font-bold text-xl">L</span>
                            </div>
                            <h1 class="text-3xl md:text-4xl font-medium text-white">
                                "Counter-P3"
                            </h1>
                        </div>
                        <p class="text-[#8b9cb8] text-sm">
                            "Powered by Leptos + WASI Preview 3 Component"
                        </p>
                    </div>

                    <div class="relative">
                        <div class="bg-[#1a2332] rounded-lg p-8 border border-[#3a4a5c]">
                            <div class="text-5xl md:text-6xl font-light text-white tabular-nums">
                                {display_count}
                            </div>
                            <div class="text-[#8b9cb8] text-sm mt-2 uppercase tracking-wider">
                                "COUNT VALUE"
                            </div>
                        </div>

                        <Show when=move || increment_action.pending().get()>
                            <div class="absolute inset-0 flex items-center justify-center bg-[#1a2332]/50 rounded-lg">
                                <div class="animate-spin rounded-full h-8 w-8 border-2 border-transparent border-t-[#00d4aa]"></div>
                            </div>
                        </Show>
                    </div>

                    <ActionForm action=increment_action>
                        <button
                            disabled=move || increment_action.pending().get()
                            class="w-full rounded-lg bg-[#00d4aa] px-6 py-3 text-[#1a2332] font-medium transition-all duration-200 hover:bg-[#00b894] active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed disabled:hover:bg-[#00d4aa]"
                        >
                            {move || if increment_action.pending().get() {
                                "Updating..."
                            } else {
                                "Increment Counter"
                            }}
                        </button>
                    </ActionForm>

                    <div class="flex items-center justify-center gap-2 text-xs">
                        <div class={move || {
                            if optimistic_count.get().is_none() {
                                "w-2 h-2 rounded-full bg-yellow-500 animate-pulse"
                            } else if increment_action.pending().get() {
                                "w-2 h-2 rounded-full bg-[#00d4aa] animate-pulse"
                            } else {
                                "w-2 h-2 rounded-full bg-[#00d4aa]"
                            }
                        }}>
                        </div>
                        <span class="text-[#8b9cb8] uppercase tracking-wider">
                            {move || {
                                if optimistic_count.get().is_none() {
                                    "Loading"
                                } else if increment_action.pending().get() {
                                    "Syncing"
                                } else {
                                    "Ready"
                                }
                            }}
                        </span>
                    </div>

                    <div class="pt-4 border-t border-[#3a4a5c]">
                        <p class="text-[#8b9cb8] text-xs">
                            "Running on raw Wasmtime serve"
                        </p>
                    </div>
                </div>
            </div>
        </div>
    }
}

#[component]
fn NotFound() -> impl IntoView {
    #[cfg(feature = "ssr")]
    {
        if let Some(resp) = use_context::<leptos_wasi::response::ResponseOptions>() {
            resp.set_status(leptos_wasi::prelude::StatusCode::NOT_FOUND);
        }
    }

    view! { <h1>"Not Found"</h1> }
}

#[cfg(feature = "ssr")]
mod storage {
    #[cfg(runtime_spin)]
    pub async fn get(key: &str) -> Result<Option<Vec<u8>>, String> {
        use spin_sdk::key_value::Store;
        let store = Store::open_default().await
            .map_err(|e| format!("Failed to open Spin KV store: {}", e))?;
        store.get(key).await
            .map_err(|e| format!("Failed to get from Spin KV: {}", e))
    }

    #[cfg(runtime_spin)]
    pub async fn set(key: &str, value: &[u8]) -> Result<(), String> {
        use spin_sdk::key_value::Store;
        let store = Store::open_default().await
            .map_err(|e| format!("Failed to open Spin KV store: {}", e))?;
        store.set(key, value).await
            .map_err(|e| format!("Failed to set in Spin KV: {}", e))
    }

    #[cfg(not(runtime_spin))]
    pub async fn get(key: &str) -> Result<Option<Vec<u8>>, String> {
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
            .map_err(|e| format!("Failed to read file: {}", e))
    }

    #[cfg(not(runtime_spin))]
    pub async fn set(key: &str, value: &[u8]) -> Result<(), String> {
        use std::fs;
        use std::path::Path;

        let base_path = std::env::var("STORAGE_PATH").unwrap_or_else(|_| "./data".to_string());
        let dir_path = Path::new(&base_path);
        if !dir_path.exists() {
            fs::create_dir_all(dir_path)
                .map_err(|e| format!("Failed to create directory: {}", e))?;
        }

        let file_path = format!("{}/{}.txt", base_path, key);
        fs::write(&file_path, value)
            .map_err(|e| format!("Failed to write file: {}", e))
    }
}

#[server(prefix = "/api")]
pub async fn get_count() -> Result<u32, ServerFnError<String>> {
    match storage::get("counter").await {
        Ok(Some(value)) => {
            let count_str = String::from_utf8(value)
                .map_err(|e| ServerFnError::ServerError(format!("Invalid UTF-8: {}", e)))?;
            let count = count_str.parse::<u32>().unwrap_or(0);
            println!("Retrieved count: {count}");
            Ok(count)
        }
        Ok(None) => {
            println!("No count found, returning 0");
            Ok(0)
        }
        Err(e) => {
            eprintln!("Error reading counter: {}", e);
            Ok(0)
        }
    }
}

#[server(prefix = "/api")]
pub async fn increment_count() -> Result<(), ServerFnError<String>> {
    let current_count = get_count().await?;
    let new_count = current_count + 1;
    println!("Incrementing count from {current_count} to {new_count}");

    storage::set("counter", new_count.to_string().as_bytes()).await
        .map_err(|e| ServerFnError::ServerError(e))?;

    Ok(())
}
