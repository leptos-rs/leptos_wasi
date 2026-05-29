use crate::app::{
    App, CustomTest, FormSubmitTest, GenericTest, GetTest, LargeBodyTest,
    MalformedRedirectTest, PanicTest, PostTest, shell,
};
use leptos::config::get_configuration;
use leptos_wasi::prelude::Handler;

type Body = axum_core::body::Body;

pub trait HandlerExt {
    fn with_server_fn<T>(self) -> Self
    where
        T: server_fn::ServerFn + 'static,
        <T as server_fn::ServerFn>::Server:
            leptos_wasi::handler::ServerWithBody<
                    <T as server_fn::ServerFn>::Error,
                    <T as server_fn::ServerFn>::InputStreamError,
                    <T as server_fn::ServerFn>::OutputStreamError,
                    ReqBody = axum_core::body::Body,
                    ResBody = axum_core::body::Body,
                >;
}

impl HandlerExt for Handler {
    fn with_server_fn<T>(self) -> Self
    where
        T: server_fn::ServerFn + 'static,
        <T as server_fn::ServerFn>::Server:
            leptos_wasi::handler::ServerWithBody<
                    <T as server_fn::ServerFn>::Error,
                    <T as server_fn::ServerFn>::InputStreamError,
                    <T as server_fn::ServerFn>::OutputStreamError,
                    ReqBody = axum_core::body::Body,
                    ResBody = axum_core::body::Body,
                >,
    {
        self.with_server_fn_axum::<T>()
    }
}

pub trait HandlerExtCustom {
    fn with_server_fn<T, B>(self) -> Self
    where
        T: server_fn::ServerFn + 'static,
        B: 'static,
        <T as server_fn::ServerFn>::Server:
            leptos_wasi::handler::ServerWithBody<
                    <T as server_fn::ServerFn>::Error,
                    <T as server_fn::ServerFn>::InputStreamError,
                    <T as server_fn::ServerFn>::OutputStreamError,
                    ReqBody = B,
                    ResBody = B,
                >,
        B: Into<leptos_wasi::response::Body> + From<bytes::Bytes> + 'static;
}

impl HandlerExtCustom for Handler {
    fn with_server_fn<T, B>(self) -> Self
    where
        T: server_fn::ServerFn + 'static,
        B: 'static,
        <T as server_fn::ServerFn>::Server:
            leptos_wasi::handler::ServerWithBody<
                    <T as server_fn::ServerFn>::Error,
                    <T as server_fn::ServerFn>::InputStreamError,
                    <T as server_fn::ServerFn>::OutputStreamError,
                    ReqBody = B,
                    ResBody = B,
                >,
        B: Into<leptos_wasi::response::Body> + From<bytes::Bytes> + 'static,
    {
        self.with_server_fn::<T>()
    }
}

fn serve_static_files(path: String) -> Option<leptos_wasi::response::Body> {
    use std::fs;
    let path = path.strip_prefix("/").unwrap_or(&path);
    if path.contains("..") || path.contains('\\') {
        return None;
    }
    // Files are served from /static in the virtual FS
    let file_path = format!("/static/{}", path);
    if let Ok(bytes) = fs::read(&file_path) {
        Some(leptos_wasi::response::Body::Sync(bytes.into()))
    } else {
        None
    }
}

// ==================== WASIp3 implementation ====================
#[cfg(feature = "wasip3")]
struct LeptosServer;

#[cfg(feature = "wasip3")]
impl wasip3::exports::http::handler::Guest for LeptosServer {
    async fn handle(
        request: wasip3::http::types::Request,
    ) -> Result<wasip3::http::types::Response, wasip3::http::types::ErrorCode>
    {
        let _ = leptos_wasi::executor::init_wasip3_spawner();

        let conf = get_configuration(None).unwrap();
        let leptos_options = conf.leptos_options;

        let req = wasip3::http_compat::http_from_wasi_request(request)?;

        let wasi_res = Handler::build(req)
            .await
            .map_err(|e| {
                eprintln!("Error building handler: {:?}", e);
                wasip3::http::types::ErrorCode::InternalError(None)
            })?
            .static_files_handler("/static", serve_static_files)
            .with_server_fn::<GetTest>()
            .with_server_fn::<PostTest>()
            .with_server_fn_generic::<GenericTest>();

        let wasi_res =
            HandlerExtCustom::with_server_fn::<CustomTest, Body>(wasi_res)
                .with_server_fn::<LargeBodyTest>()
                .with_server_fn::<PanicTest>()
                .with_server_fn::<FormSubmitTest>()
                .with_server_fn::<MalformedRedirectTest>()
                .generate_routes(App)
                .handle_with_context(
                    move || shell(leptos_options.clone()),
                    || {},
                )
                .await
                .map_err(|e| {
                    eprintln!("Error handling request: {:?}", e);
                    wasip3::http::types::ErrorCode::InternalError(None)
                })?;

        Ok(wasi_res)
    }
}

#[cfg(feature = "wasip3")]
wasip3::http::service::export!(LeptosServer);

// ==================== WASIp2 implementation ====================
#[cfg(all(feature = "wasip2", not(feature = "wasip3")))]
struct LeptosServer;

#[cfg(all(feature = "wasip2", not(feature = "wasip3")))]
impl wasi::exports::wasi::http::incoming_handler::Guest for LeptosServer {
    fn handle(
        request: wasi::exports::wasi::http::incoming_handler::IncomingRequest,
        response_out: wasi::exports::wasi::http::incoming_handler::ResponseOutparam,
    ) {
        use any_spawner::Executor as LeptosExecutor;
        use leptos_wasi::prelude::WasiExecutor;

        let executor = WasiExecutor::new(leptos_wasi::executor::Mode::Stalled);
        LeptosExecutor::init_local_custom_executor(executor.clone()).unwrap();

        let conf = get_configuration(None).unwrap();
        let leptos_options = conf.leptos_options;

        executor.run_until(async {
            let h = Handler::build(request, response_out)
                .unwrap()
                .static_files_handler("/static", serve_static_files)
                .with_server_fn::<GetTest>()
                .with_server_fn::<PostTest>()
                .with_server_fn_generic::<GenericTest>();

            let h = HandlerExtCustom::with_server_fn::<CustomTest, Body>(h)
                .with_server_fn::<LargeBodyTest>()
                .with_server_fn::<PanicTest>()
                .with_server_fn::<FormSubmitTest>()
                .with_server_fn::<MalformedRedirectTest>()
                .generate_routes(App)
                .handle_with_context(
                    move || shell(leptos_options.clone()),
                    || {},
                )
                .await;
            h.unwrap();
        });
    }
}

#[cfg(all(feature = "wasip2", not(feature = "wasip3")))]
wasi::http::proxy::export!(LeptosServer with_types_in wasi);
