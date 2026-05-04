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
    // Provides context that manages stylesheets, titles, meta tags, etc.
    provide_meta_context();

    let fallback = || view! { "Page not found." }.into_view();

    view! {
        <Stylesheet id="leptos" href="/pkg/spin_counter.css" />
        <Meta
            name="description"
            content="A website running its server-side as a WASI Component :D"
        />

        <Title text="Welcome to Leptos X Spin!" />

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

/// Renders the home page of your application.
#[component]
fn HomePage() -> impl IntoView {
    let increment_action = ServerAction::<IncrementCount>::new();

    // Local optimistic count state
    let (optimistic_count, set_optimistic_count) = signal(None::<u32>);

    // Server count resource
    let count = Resource::new(move || increment_action.version().get(), |_| get_count());

    // Initialize from localStorage or server
    Effect::new(move |_| {
        if optimistic_count.get().is_none() {
            // Try to get from localStorage first
            #[cfg(feature = "hydrate")]
            {
                if let Some(window) = window() {
                    if let Ok(Some(storage)) = window.local_storage() {
                        if let Ok(Some(cached_count_str)) = storage.get_item("spin_counter_count") {
                            if let Ok(cached_count) = cached_count_str.parse::<u32>() {
                                set_optimistic_count.set(Some(cached_count));
                                return;
                            }
                        }
                    }
                }
            }

            // Fallback to server count
            if let Some(Ok(server_count)) = count.get() {
                set_optimistic_count.set(Some(server_count));

                // Cache in localStorage
                #[cfg(feature = "hydrate")]
                {
                    if let Some(window) = window() {
                        if let Ok(Some(storage)) = window.local_storage() {
                            let _ = storage.set_item("spin_counter_count", &server_count.to_string());
                        }
                    }
                }
            }
        }
    });

    // Sync server updates to localStorage
    Effect::new(move |_| {
        if let Some(Ok(server_count)) = count.get() {
            // Only update if we have a successful server response and it's different
            if let Some(current_optimistic) = optimistic_count.get() {
                if server_count != current_optimistic {
                    set_optimistic_count.set(Some(server_count));
                }
            }

            // Always update localStorage with server value
            #[cfg(feature = "hydrate")]
            {
                if let Some(window) = window() {
                    if let Ok(Some(storage)) = window.local_storage() {
                        let _ = storage.set_item("spin_counter_count", &server_count.to_string());
                    }
                }
            }
        }
    });

    // Optimistic increment
    let on_click = move |_| {
        // Immediately update UI
        let new_count = optimistic_count.get().unwrap_or(0) + 1;
        set_optimistic_count.set(Some(new_count));

        // Update localStorage immediately
        #[cfg(feature = "hydrate")]
        {
            if let Some(window) = window() {
                if let Ok(Some(storage)) = window.local_storage() {
                    let _ = storage.set_item("spin_counter_count", &new_count.to_string());
                }
            }
        }

        // Trigger server action
        increment_action.dispatch(IncrementCount {});
    };

    view! {
        <div class="min-h-screen bg-[#1a2332] flex items-center justify-center p-4">
            <div class="bg-[#263343] rounded-xl shadow-2xl p-8 md:p-12 max-w-md w-full border border-[#3a4a5c]">
                <div class="text-center space-y-8">
                    // Header
                    <div class="space-y-2">
                        <div class="flex items-center justify-center gap-3 mb-4">
                            // Fermyon-style logo placeholder
                            <div class="w-10 h-10 bg-[#00d4aa] rounded-lg flex items-center justify-center">
                                <span class="text-[#1a2332] font-bold text-xl">C</span>
                            </div>
                            <h1 class="text-3xl md:text-4xl font-medium text-white">
                                "spin-counter"
                            </h1>
                        </div>
                        <p class="text-[#8b9cb8] text-sm">
                            "Powered by Fermyon Spin + Leptos + WASM"
                        </p>
                    </div>

                    // Counter Display
                    <div class="relative">
                        <div class="bg-[#1a2332] rounded-lg p-8 border border-[#3a4a5c]">
                            <div class="text-5xl md:text-6xl font-light text-white tabular-nums">
                                {move || {
                                    optimistic_count.get()
                                        .map(|c| c.to_string())
                                        .unwrap_or_else(|| "...".to_string())
                                }}
                            </div>
                            <div class="text-[#8b9cb8] text-sm mt-2 uppercase tracking-wider">
                                "Count Value"
                            </div>
                        </div>

                        // Loading indicator overlay
                        <Show when=move || increment_action.pending().get()>
                            <div class="absolute inset-0 flex items-center justify-center bg-[#1a2332]/50 rounded-lg">
                                <div class="animate-spin rounded-full h-8 w-8 border-2 border-transparent border-t-[#00d4aa]"></div>
                            </div>
                        </Show>
                    </div>

                    // Button
                    <button
                        on:click=on_click
                        disabled=move || increment_action.pending().get()
                        class="w-full rounded-lg bg-[#00d4aa] px-6 py-3 text-[#1a2332] font-medium transition-all duration-200 hover:bg-[#00b894] active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed disabled:hover:bg-[#00d4aa]"
                    >
                        {move || if increment_action.pending().get() {
                            "Updating..."
                        } else {
                            "Increment Counter"
                        }}
                    </button>

                    // Status indicators
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

                    // Footer info
                    <div class="pt-4 border-t border-[#3a4a5c]">
                        <p class="text-[#8b9cb8] text-xs">
                            "Running on Fermyon Cloud"
                        </p>
                    </div>
                </div>
            </div>
        </div>
    }
}

/// 404 - Not Found
#[component]
fn NotFound() -> impl IntoView {
    // set an HTTP status code 404
    // this is feature gated because it can only be done during
    // initial server-side rendering
    // if you navigate to the 404 page subsequently, the status
    // code will not be set because there is not a new HTTP request
    // to the server
    #[cfg(feature = "ssr")]
    {
        // this can be done inline because it's synchronous
        // if it were async, we'd use a server function
        if let Some(resp) = use_context::<leptos_wasi::response::ResponseOptions>() {
            resp.set_status(leptos_wasi::prelude::StatusCode::NOT_FOUND);
        }
    }

    view! { <h1>"Not Found"</h1> }
}

#[server(prefix = "/api")]
pub async fn get_count() -> Result<u32, ServerFnError<String>> {
    let store = spin_sdk::key_value::Store::open_default().map_err(|e| e.to_string())?;
    match store.get_json::<u32>("spin_counter_count") {
        Ok(Some(count)) => {
            println!("Retrieved count: {count}");
            Ok(count)
        }
        Ok(None) => {
            println!("No count found, returning 0");
            Ok(0)
        }
        Err(e) => {
            eprintln!("Error retrieving count: {e}");
            Ok(0)
        }
    }
}

#[server(prefix = "/api")]
pub async fn increment_count() -> Result<(), ServerFnError<String>> {
    let store = spin_sdk::key_value::Store::open_default().map_err(|e| e.to_string())?;

    // Get current count
    let current_count = match store.get_json::<u32>("spin_counter_count") {
        Ok(Some(count)) => count,
        Ok(None) => 0,
        Err(_) => 0,
    };

    let new_count = current_count + 1;
    println!("Incrementing count from {current_count} to {new_count}");

    store
        .set_json("spin_counter_count", &new_count)
        .map_err(|e| ServerFnError::ServerError(e.to_string()))?;

    Ok(())
}
