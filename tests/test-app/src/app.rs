use leptos::prelude::*;
use leptos_meta::*;
use leptos_router::{
    SsrMode,
    components::{Route, Router, Routes},
    path,
};
use server_fn::codec::{GetUrl, Json};

pub fn shell(options: LeptosOptions) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1" />
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
        <Stylesheet id="leptos" href="/static/app.css" />
        <Router>
            <main>
                <Routes fallback>
                    <Route path=path!("/ssr/async") ssr=SsrMode::Async view=SsrAsyncView />
                    <Route path=path!("/ssr/in-order") ssr=SsrMode::InOrder view=SsrInOrderView />
                    <Route path=path!("/ssr/out-of-order") ssr=SsrMode::OutOfOrder view=SsrOutOfOrderView />
                    <Route path=path!("/ssr/meta") ssr=SsrMode::Async view=SsrMetaView />
                    <Route path=path!("/ssr/panic") ssr=SsrMode::Async view=SsrPanicView />
                    <Route path=path!("/static-page") ssr=SsrMode::Async view=StaticPageSsrView />
                    <Route path=path!("/*any") view=NotFound />
                </Routes>
            </main>
        </Router>
    }
}

#[component]
fn SsrAsyncView() -> impl IntoView {
    let resource = Resource::new(
        || (),
        |_| async { "Async resource resolved".to_string() },
    );
    view! {
        <div>
            <h1>"Async View"</h1>
            <Suspense fallback=move || view! { <p>"Loading..."</p> }>
                <p>{move || resource.get()}</p>
            </Suspense>
        </div>
    }
}

#[component]
fn SsrInOrderView() -> impl IntoView {
    let resource = Resource::new(
        || (),
        |_| async { "InOrder resource resolved".to_string() },
    );
    view! {
        <div>
            <h1>"InOrder View"</h1>
            <Suspense fallback=move || view! { <p>"Loading..."</p> }>
                <p>{move || resource.get()}</p>
            </Suspense>
        </div>
    }
}

#[component]
fn SsrOutOfOrderView() -> impl IntoView {
    let resource = Resource::new(
        || (),
        |_| async { "OutOfOrder resource resolved".to_string() },
    );
    view! {
        <div>
            <h1>"OutOfOrder View"</h1>
            <Suspense fallback=move || view! { <p>"Loading..."</p> }>
                <p>{move || resource.get()}</p>
            </Suspense>
        </div>
    }
}

#[component]
fn SsrMetaView() -> impl IntoView {
    view! {
        <Title text="Meta Test Title" />
        <Meta name="description" content="Meta Test Description" />
        <div>
            <h1>"Meta View"</h1>
        </div>
    }
}

#[component]
fn SsrPanicView() -> impl IntoView {
    let should_panic = true;
    if should_panic {
        panic!("SsrPanicView-panic");
    }
    view! {
        <div>"Will not render"</div>
    }
}

#[component]
fn NotFound() -> impl IntoView {
    if let Some(resp) = use_context::<leptos_wasi::response::ResponseOptions>()
    {
        resp.set_status(leptos_wasi::prelude::StatusCode::NOT_FOUND);
    }
    view! { <h1>"Not Found"</h1> }
}

// Server functions:
#[server(input = GetUrl, prefix = "/api", endpoint = "get_test")]
pub async fn get_test() -> Result<String, ServerFnError> {
    Ok("GET response".to_string())
}

#[server(input = Json, prefix = "/api", endpoint = "post_test")]
pub async fn post_test(msg: String) -> Result<String, ServerFnError> {
    Ok(format!("POST response: {}", msg))
}

pub struct MyGenericServerBackend;

impl<Error, InputStreamError, OutputStreamError>
    server_fn::server::Server<Error, InputStreamError, OutputStreamError>
    for MyGenericServerBackend
where
    Error: server_fn::error::FromServerFnError + Send + Sync + 'static,
    InputStreamError:
        server_fn::error::FromServerFnError + Send + Sync + 'static,
    OutputStreamError:
        server_fn::error::FromServerFnError + Send + Sync + 'static,
{
    type Request = http::Request<axum_core::body::Body>;
    type Response = http::Response<axum_core::body::Body>;

    fn spawn(
        future: impl std::future::Future<Output = ()> + Send + 'static,
    ) -> Result<(), Error> {
        #[cfg(target_arch = "wasm32")]
        {
            std::mem::drop(future);
            Ok(())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            tokio::spawn(future);
            Ok(())
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct GenericTest {}

impl server_fn::ServerFn for GenericTest {
    const PATH: &'static str = "/api/generic_test";
    type Client = <PostTest as server_fn::ServerFn>::Client;
    type Server = MyGenericServerBackend;
    type Protocol = <PostTest as server_fn::ServerFn>::Protocol;
    type Output = String;
    type Error = server_fn::ServerFnError;
    type InputStreamError = server_fn::ServerFnError;
    type OutputStreamError = server_fn::ServerFnError;

    async fn run_body(self) -> Result<Self::Output, Self::Error> {
        Ok("Generic response".to_string())
    }
}

#[server(input = Json, prefix = "/api", endpoint = "custom_test")]
pub async fn custom_test() -> Result<String, ServerFnError> {
    Ok("Custom response".to_string())
}

#[server(input = Json, prefix = "/api", endpoint = "large_body_test")]
pub async fn large_body_test(data: String) -> Result<usize, ServerFnError> {
    Ok(data.len())
}

#[server(input = Json, prefix = "/api", endpoint = "panic_test")]
pub async fn panic_test() -> Result<String, ServerFnError> {
    panic!("test-panic-endpoint");
}

#[server(prefix = "/api", endpoint = "form_submit_test")]
pub async fn form_submit_test() -> Result<(), ServerFnError> {
    // Does not set Location, returns Ok
    Ok(())
}

#[component]
pub fn StaticPageSsrView() -> impl IntoView {
    view! { <h1>"Static Page SSR View"</h1> }
}

#[server(input = Json, prefix = "/api", endpoint = "malformed_redirect_test")]
pub async fn malformed_redirect_test() -> Result<(), ServerFnError> {
    leptos_wasi::utils::redirect("/target-page\r\nLocation: http://evil.com");
    Ok(())
}
