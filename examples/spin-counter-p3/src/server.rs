use leptos::config::get_configuration;
use leptos_wasi::executor::init_wasip3_spawner;
use spin_sdk::http::{Request, Response, IntoResponse};
use spin_sdk::http_service;

use crate::app::{shell, App, GetCount, IncrementCount};

#[http_service]
async fn handle_request(req: Request) -> Result<impl IntoResponse, anyhow::Error> {
    let _ = init_wasip3_spawner();
    handle_request_inner(req).await
}

async fn handle_request_inner(
    request: Request,
) -> Result<Response, anyhow::Error> {
    use leptos_wasi::prelude::Handler;

    let conf = get_configuration(None).unwrap();
    let leptos_options = conf.leptos_options;

    let wasi_res = Handler::build(request).await?
        .with_server_fn::<GetCount, _>()
        .with_server_fn::<IncrementCount, _>()
        .generate_routes(App)
        .handle_with_context(move || shell(leptos_options.clone()), || {})
        .await?;

    let res = wasip3::http_compat::http_from_wasi_response(wasi_res)
        .map_err(|e| anyhow::anyhow!("WASI HTTP conversion error: {:?}", e))?;

    Ok(res)
}
