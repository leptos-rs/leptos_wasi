#![forbid(unsafe_code)]

#[cfg(all(feature = "wasip2", not(feature = "wasip3")))]
use crate::CHUNK_BYTE_SIZE;
use crate::{
    response::{Response, ResponseOptions},
    utils::redirect,
};

/// Maximum size for request bodies when collecting async streams (16MB)
/// This prevents memory exhaustion from malicious or very large requests
pub(crate) const MAX_REQUEST_BODY_SIZE: usize = 16 * 1024 * 1024;
use bytes::Bytes;
#[cfg(all(feature = "wasip2", not(feature = "wasip3")))]
use futures::stream;
use futures::{StreamExt, stream::once};
use http::{
    HeaderValue, Request, StatusCode, Uri,
    header::{ACCEPT, LOCATION, REFERER},
    request::Parts,
};
use hydration_context::SsrSharedContext;
use leptos::{
    IntoView,
    prelude::{Owner, ScopedFuture, provide_context},
};
use leptos_integration_utils::{ExtendResponse, PinnedStream};
use leptos_meta::ServerMetaContext;
use leptos_router::{
    ExpandOptionals, PathSegment, RouteList, RouteListing, SsrMode,
    components::provide_server_redirect, location::RequestUrl,
};
use mime_guess::MimeGuess;
use routefinder::Router;
use server_fn::{Protocol, ServerFn, response::generic::Body};
use std::{future::Future, pin::Pin, sync::Arc};
use thiserror::Error;
#[cfg(all(feature = "wasip2", not(feature = "wasip3")))]
use wasi::http::types::{
    IncomingRequest, OutgoingBody, OutgoingResponse, ResponseOutparam,
};

/// We use a type-erased alias to define the server function handler's type.
/// It replaces `ServerFnTraitObj` because that type has strict constraints.
/// Takes a Request<Body> and returns a pinned future that outputs Response<Body>
type ServerFnHandler = Box<
    dyn Fn(
            Request<Body>,
        )
            -> Pin<Box<dyn Future<Output = http::Response<Body>> + Send>>
        + Send,
>;

/// Handle routing, static file serving and response tx using the low-level
/// `wasi:http` APIs.
///
/// ## Performance Considerations
///
/// This handler is optimised for the special case of WASI Components being spawned
/// on a per-request basis. That is, the lifetime of the component is bound to the
/// one of the request, so we don't do any fancy pre-setup: it means
/// **your Server-Side will always be cold-started**.
///
/// While it could have a bad impact on the performance of your app, please, know
/// that there is a *shotcut* mechanism implemented that allows the [`Handler`]
/// to shortcut the whole HTTP Rendering and Reactivity logic to directly jump to
/// writting the response in those case:
///
/// * The user request a static-file, then, calling [`Handler::static_files_handler`]
///   will *shortcut* the handler and all future calls are ignored to reach
///   [`Handler::handle_with_context`] *almost* instantly.
/// * The user reach a server function, then, calling [`Handler::with_server_fn`]
///   will check if the request's path matches the one from the passed server functions,
///   if so, *shortcut* the handler.
///
/// This implementation ensures that, even though your component is cold-started
/// on each request, the performance are good. Please, note that this approach is
/// directly enabled by the fact WASI Components have under-millisecond start-up
/// times! It wouldn't be practical to do that with traditional container-based solutions.
///
/// ## Limitations
///
/// [`SsrMode::Static`] is not implemented yet, having one in your `<Router>`
/// will cause `Handler::handle_with_context` to panic!
///
/// # Examples
///
/// ```ignore
/// use leptos::prelude::get_configuration;
/// use leptos_wasi::prelude::{Handler, WasiExecutor};
/// use any_spawner::Executor;
/// use wasi::exports::http::incoming_handler::{Guest, IncomingRequest, ResponseOutparam};
///
/// struct MyServer;
///
/// impl Guest for MyServer {
///     fn handle(request: IncomingRequest, response_out: ResponseOutparam) {
///         let executor = WasiExecutor::new(leptos_wasi::executor::Mode::Stalled);
///         Executor::init_local_custom_executor(executor.clone()).unwrap();
///
///         executor.run_until(async {
///             let conf = get_configuration(None).unwrap();
///             let opt = conf.leptos_options;
///
///             Handler::build(request, response_out).unwrap()
///                 .generate_routes(App)
///                 .handle_with_context(move || shell(opt.clone()), || {})
///                 .await
///                 .unwrap();
///         });
///     }
/// }
/// ```
#[cfg(all(feature = "wasip2", not(feature = "wasip3")))]
pub struct Handler {
    req: Request<Bytes>,
    res_out: ResponseOutparam,

    // *shortcut* if any is set
    server_fn: Option<ServerFnHandler>,
    preset_res: Option<Response>,
    should_404: bool,

    // built using the user-defined app_fn
    ssr_router: Router<RouteListing>,
}

#[cfg(all(feature = "wasip2", not(feature = "wasip3")))]
impl Handler {
    /// Wraps the WASI Preview 2 resources to handle the request.
    ///
    /// Could fail if the [`IncomingRequest`] cannot be converted to
    /// a [`http::Request`].
    ///
    /// # Example
    ///
    /// ```ignore
    /// let handler = Handler::build(request, response_out)?;
    /// ```
    pub fn build(
        req: IncomingRequest,
        res_out: ResponseOutparam,
    ) -> Result<Self, HandlerError> {
        match crate::request::Request(req).try_into() {
            Ok(http_req) => Ok(Self {
                req: http_req,
                res_out,
                server_fn: None,
                preset_res: None,
                ssr_router: Router::new(),
                should_404: false,
            }),
            Err(crate::request::RequestError::BodyTooLarge(limit)) => {
                let error_msg =
                    format!("Request body too large (max: {} bytes)", limit);
                let mut res = http::Response::new(crate::response::Body::Sync(
                    Bytes::from(error_msg),
                ));
                *res.status_mut() = StatusCode::PAYLOAD_TOO_LARGE;
                Ok(Self {
                    req: http::Request::new(Bytes::new()),
                    res_out,
                    server_fn: None,
                    preset_res: Some(res.into()),
                    ssr_router: Router::new(),
                    should_404: false,
                })
            }
            Err(e) => Err(HandlerError::Request(e)),
        }
    }

    // Test whether we are ready to send a response to shortcut some
    // code and provide a fast-path.
    #[inline]
    const fn shortcut(&self) -> bool {
        self.server_fn.is_some() || self.preset_res.is_some() || self.should_404
    }

    /// Tests if the request path matches the bound server function
    /// and *shortcut* the [`Handler`] to quickly reach
    /// the call to [`Handler::handle_with_context`].
    ///
    /// # Request Body Support
    /// Fully supports both synchronous and asynchronous request bodies:
    /// - Sync bodies: Passed through directly for optimal performance
    /// - Async bodies: Automatically collected (max 16MB) with proper error handling
    ///
    /// Note: You only need to specify the server function type:
    /// `.with_server_fn::<MyServerFn>()`
    ///
    /// For most use cases, prefer the convenience methods:
    /// - `.with_server_fn_axum::<MyServerFn>()` (most common)
    /// - `.with_server_fn_generic::<MyServerFn>()` (other backends)
    ///
    /// # Example
    ///
    /// ```ignore
    /// let handler = handler.with_server_fn::<MyServerFn>();
    /// ```
    pub fn with_server_fn<T>(mut self) -> Self
    where
        T: ServerFn + 'static,
        T::Server:
            ServerWithBody<T::Error, T::InputStreamError, T::OutputStreamError>,
        ReqBody<T>: Into<crate::response::Body> + From<Bytes> + 'static,
        ResBody<T>: Into<crate::response::Body> + 'static,
    {
        if self.shortcut() {
            return self;
        }

        if self.req.method()
            == <T::Protocol as Protocol<
                T,
                T::Output,
                T::Client,
                T::Server,
                T::Error,
                T::InputStreamError,
                T::OutputStreamError,
            >>::METHOD
            && self.req.uri().path() == T::PATH
        {
            // We can't use ServerFnTraitObj::new due to type constraints
            // Instead, create a boxed function that calls the server function
            self.server_fn = Some(Box::new(move |request| {
                Box::pin(async move {
                    // Convert Request<Body> to Request<ServerBody>
                    let (parts, body) = request.into_parts();
                    let server_body = match body {
                        Body::Sync(bytes) => {
                            if bytes.len() > MAX_REQUEST_BODY_SIZE {
                                let error_msg = format!(
                                    "Request body too large (max: {} bytes)",
                                    MAX_REQUEST_BODY_SIZE
                                );
                                let error_response = http::Response::builder()
                                    .status(413) // Payload Too Large
                                    .body(Body::Sync(Bytes::from(error_msg)))
                                    .unwrap();
                                return error_response;
                            }
                            ReqBody::<T>::from(bytes)
                        }
                        Body::Async(mut stream) => {
                            // Collect the async stream into bytes
                            // This is necessary because server functions expect a complete body
                            use futures::StreamExt;
                            let mut collected_bytes = Vec::new();

                            while let Some(chunk_result) = stream.next().await {
                                match chunk_result {
                                    Ok(chunk) => {
                                        // Check size limit before adding chunk
                                        if collected_bytes.len() + chunk.len()
                                            > MAX_REQUEST_BODY_SIZE
                                        {
                                            let error_msg = format!(
                                                "Request body too large (max: \
                                                 {} bytes)",
                                                MAX_REQUEST_BODY_SIZE
                                            );
                                            let error_response =
                                                http::Response::builder()
                                                    .status(413) // Payload Too Large
                                                    .body(Body::Sync(
                                                        Bytes::from(error_msg),
                                                    ))
                                                    .unwrap();
                                            return error_response;
                                        }
                                        collected_bytes
                                            .extend_from_slice(&chunk);
                                    }
                                    Err(e) => {
                                        // Handle stream errors by returning an error response
                                        let error_msg = format!(
                                            "Failed to read request body: {}",
                                            e
                                        );
                                        let error_response =
                                            http::Response::builder()
                                                .status(400)
                                                .body(Body::Sync(Bytes::from(
                                                    error_msg,
                                                )))
                                                .unwrap();
                                        return error_response;
                                    }
                                }
                            }

                            ReqBody::<T>::from(Bytes::from(collected_bytes))
                        }
                    };

                    let server_request =
                        Request::from_parts(parts, server_body);
                    let response = T::run_on_server(server_request).await;
                    // Convert Response<ServerBody> to Response<server_fn::response::generic::Body>
                    response.map(|body| {
                        let our_body: crate::response::Body = body.into();
                        match our_body {
                            crate::response::Body::Sync(bytes) => {
                                Body::Sync(bytes)
                            }
                            crate::response::Body::Async(stream) => {
                                Body::Async(stream)
                            }
                        }
                    })
                })
            }));
        }

        self
    }

    /// Convenience method for server functions using the generic server_fn body.
    /// This works with backends that use `server_fn::response::generic::Body`.
    ///
    /// Note: Most leptos projects use the axum backend, so you probably want
    /// `with_server_fn_axum` instead.
    ///
    /// # Example
    /// ```ignore
    /// handler.with_server_fn_generic::<UpdateCount>()
    /// ```
    pub fn with_server_fn_generic<T>(self) -> Self
    where
        T: ServerFn + 'static,
        T::Server:
            ServerWithBody<T::Error, T::InputStreamError, T::OutputStreamError>,
        ReqBody<T>: Into<crate::response::Body> + From<Bytes> + 'static,
        ResBody<T>: Into<crate::response::Body> + 'static,
    {
        self.with_server_fn::<T>()
    }

    /// Convenience method for server functions using the axum backend.
    /// This is the recommended method for most leptos projects as it avoids
    /// needing to specify the body type parameter.
    ///
    /// # Request Body Handling
    /// Supports both sync and async request bodies:
    /// - Sync bodies are passed through directly
    /// - Async bodies are collected into memory (max 16MB) before processing
    ///
    /// # Example
    /// ```ignore
    /// handler.with_server_fn_axum::<UpdateCount>()
    /// ```
    /// instead of:
    /// ```ignore
    /// handler.with_server_fn::<UpdateCount>()
    /// ```
    pub fn with_server_fn_axum<T>(self) -> Self
    where
        T: ServerFn + 'static,
        T::Server: ServerWithBody<
                T::Error,
                T::InputStreamError,
                T::OutputStreamError,
                ReqBody = axum_core::body::Body,
                ResBody = axum_core::body::Body,
            >,
    {
        self.with_server_fn::<T>()
    }

    /// Registers a custom static file handler for a specific URI prefix.
    ///
    /// If the request URL starts with the prefix, the callback is executed to resolve the file.
    /// If the callback returns `None`, the response will be a 404. Otherwise, the returned
    /// [`crate::response::Body`] will be served.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let handler = handler.static_files_handler("/assets", |path| {
    ///     let file_bytes = load_from_blobstore(&path)?;
    ///     Some(leptos_wasi::response::Body::Sync(file_bytes))
    /// });
    /// ```
    pub fn static_files_handler<T>(
        mut self,
        prefix: T,
        handler: impl Fn(String) -> Option<crate::response::Body>
        + 'static
        + Send
        + Clone,
    ) -> Self
    where
        T: TryInto<Uri>,
        <T as TryInto<Uri>>::Error: std::error::Error,
    {
        if self.shortcut() {
            return self;
        }

        let req_path = self.req.uri().path();
        let prefix_uri = prefix.try_into().expect("you passed an invalid Uri");
        let prefix_path = prefix_uri.path();

        let is_match = if req_path == prefix_path {
            true
        } else if let Some(rest) = req_path.strip_prefix(prefix_path) {
            rest.starts_with('/') || prefix_path.ends_with('/')
        } else {
            false
        };

        if is_match {
            let stripped_url = req_path.strip_prefix(prefix_path).unwrap_or("");
            let trimmed_url = stripped_url.trim_start_matches('/');
            let decoded_url = url_decode(trimmed_url);

            // Security: reject path traversal attempts before
            // invoking the user-provided file handler.
            if decoded_url.contains("..") || decoded_url.contains('\\') {
                self.should_404 = true;
                return self;
            }

            match handler(decoded_url.clone()) {
                None => self.should_404 = true,
                Some(body) => {
                    let mut res = http::Response::new(body);
                    let mime = MimeGuess::from_path(&decoded_url);

                    res.headers_mut().insert(
                        http::header::CONTENT_TYPE,
                        HeaderValue::from_str(
                            mime.first_or_octet_stream().as_ref(),
                        )
                        .expect("internal error: could not parse MIME type"),
                    );

                    self.preset_res = Some(res.into());
                }
            }
        }

        self
    }

    /// Generates routes for the application from the root component.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let handler = handler.generate_routes(App);
    /// ```
    pub fn generate_routes<IV>(
        self,
        app_fn: impl Fn() -> IV + 'static + Send + Clone,
    ) -> Self
    where
        IV: IntoView + 'static,
    {
        self.generate_routes_with_exclusions_and_context(app_fn, None, || {})
    }

    /// Generates routes for the application and injects custom contexts.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let handler = handler.generate_routes_with_context(App, || {
    ///     provide_context(MyGlobalState::new());
    /// });
    /// ```
    pub fn generate_routes_with_context<IV>(
        self,
        app_fn: impl Fn() -> IV + 'static + Send + Clone,
        additional_context: impl Fn() + 'static + Send + Clone,
    ) -> Self
    where
        IV: IntoView + 'static,
    {
        self.generate_routes_with_exclusions_and_context(
            app_fn,
            None,
            additional_context,
        )
    }

    /// Generates routes for the application, excluding specific paths and injecting custom contexts.
    pub fn generate_routes_with_exclusions_and_context<IV>(
        mut self,
        app_fn: impl Fn() -> IV + 'static + Send + Clone,
        excluded_routes: Option<Vec<String>>,
        additional_context: impl Fn() + 'static + Send + Clone,
    ) -> Self
    where
        IV: IntoView + 'static,
    {
        // If we matched a server function, we do not need to go through
        // all of that.
        if self.shortcut() {
            return self;
        }

        if !self.ssr_router.is_empty() {
            panic!("generate_routes was called twice");
        }

        let owner = Owner::new_root(Some(Arc::new(SsrSharedContext::new())));
        let routes = owner
            .with(|| {
                // as we are generating the app to extract
                // the <Router/>, we want to mock the root path.
                provide_context(RequestUrl::new(""));
                let (mock_meta, _) = ServerMetaContext::new();
                let (mock_parts, _) = Request::new("").into_parts();
                provide_context(mock_meta);
                provide_context(mock_parts);
                provide_context(ResponseOptions::default());
                additional_context();
                RouteList::generate(&app_fn)
            })
            .unwrap_or_default()
            .into_inner()
            .into_iter()
            .flat_map(IntoRouteListing::into_route_listing)
            .filter(|route| {
                excluded_routes.as_ref().is_none_or(|excluded_routes| {
                    !excluded_routes.contains(&route.0)
                })
            });

        for (path, route_listing) in routes {
            self.ssr_router
                .add(path, route_listing)
                .expect("internal error: impossible to parse a RouteListing");
        }

        self
    }

    /// Consumes the [`Handler`] to execute routing, SSR rendering, and response sending
    /// under WASI Preview 2.
    ///
    /// # Example
    ///
    /// ```ignore
    /// handler.handle_with_context(
    ///     move || shell(leptos_options.clone()),
    ///     || { provide_context(db_connection.clone()); }
    /// ).await?;
    /// ```
    pub async fn handle_with_context<IV>(
        self,
        app: impl Fn() -> IV + 'static + Send + Clone,
        additional_context: impl Fn() + 'static + Clone + Send,
    ) -> Result<(), HandlerError>
    where
        IV: IntoView + 'static,
    {
        let path = self.req.uri().path().to_string();
        let best_match = self.ssr_router.best_match(&path);
        let (parts, body) = self.req.into_parts();
        let context_parts = parts.clone();
        let req = Request::from_parts(parts, body);

        let owner = Owner::new();
        let response = owner
            .with(|| {
                ScopedFuture::new(async move {
                    let res_opts = ResponseOptions::default();
                    let response: Option<Response> = if self.should_404 {
                        None
                    } else if self.preset_res.is_some() {
                        self.preset_res
                    } else if let Some(sfn) = self.server_fn {
                        provide_contexts(additional_context, context_parts, res_opts.clone());

                        // store Accepts and Referer in case we need them for redirect (below)
                        let accepts_html = req
                            .headers()
                            .get(ACCEPT)
                            .and_then(|v| v.to_str().ok())
                            .map(|v| v.contains("text/html"))
                            .unwrap_or(false);
                        let referrer = req
                            .headers()
                            .get(REFERER)
                            .or_else(|| req.headers().get("referrer"))
                            .cloned();

                        let req_with_body = req.map(Body::from);
                        let mut res = sfn(req_with_body).await;

                        let mut redirect_target = None;

                        if let (true, Some(referrer)) = (accepts_html, referrer.clone()) {
                            let is_default_redirect = res.headers().get(LOCATION)
                                .and_then(|v| v.to_str().ok())
                                == Some("/");
                            let has_location = res.headers().get(LOCATION).is_some();
                            if !has_location || is_default_redirect {
                                if let Some(sanitized) = sanitize_referrer(&referrer) {
                                    *res.status_mut() = StatusCode::FOUND;
                                    redirect_target = Some(sanitized);
                                } else if !has_location {
                                    *res.status_mut() = StatusCode::FOUND;
                                    redirect_target = Some(HeaderValue::from_static("/"));
                                }
                            }
                        }

                        if let (None, Some(location)) = (redirect_target.as_ref(), res.headers().get(LOCATION).cloned()) {
                            let sanitized = sanitize_referrer(&location)
                                .unwrap_or_else(|| HeaderValue::from_static("/"));
                            redirect_target = Some(sanitized);
                        }

                        if let Some(target) = redirect_target {
                            res.headers_mut().insert(LOCATION, target);
                        }

                        Some(res.into())
                    } else if let Some(best_match) = best_match {
                        let listing = best_match.handler();
                        let (meta_context, meta_output) = ServerMetaContext::new();

                        let add_ctx = additional_context.clone();
                        let additional_context = {
                            let res_opts = res_opts.clone();
                            let meta_ctx = meta_context.clone();
                            move || {
                                provide_contexts(add_ctx, context_parts, res_opts);
                                provide_context(meta_ctx);
                            }
                        };

                        Some(
                            Response::from_app(
                                app,
                                meta_output,
                                additional_context,
                                res_opts.clone(),
                                match listing.mode() {
                                    SsrMode::Async => |app, chunks, _| {
                                        Box::pin(async move {
                                            let app = if cfg!(feature = "islands-router") {
                                                app.to_html_stream_in_order_branching()
                                            } else {
                                                app.to_html_stream_in_order()
                                            };
                                            let app = app.collect::<String>().await;
                                            let chunks = chunks();
                                            Box::pin(once(async move { app }).chain(chunks))
                                                as PinnedStream<String>
                                        })
                                    },
                                    SsrMode::InOrder => |app, chunks, _| {
                                        Box::pin(async move {
                                            let app = if cfg!(feature = "islands-router") {
                                                app.to_html_stream_in_order_branching()
                                            } else {
                                                app.to_html_stream_in_order()
                                            };
                                            Box::pin(app.chain(chunks())) as PinnedStream<String>
                                        })
                                    },
                                    SsrMode::PartiallyBlocked | SsrMode::OutOfOrder => {
                                        |app, chunks, _| {
                                            Box::pin(async move {
                                                let app = if cfg!(feature = "islands-router") {
                                                    app.to_html_stream_out_of_order_branching()
                                                } else {
                                                    app.to_html_stream_out_of_order()
                                                };
                                                Box::pin(app.chain(chunks()))
                                                    as PinnedStream<String>
                                            })
                                        }
                                    }
                                    SsrMode::Static(_) => {
                                        panic!("SsrMode::Static routes are not supported yet!")
                                    }
                                },
                                // Add the 6th parameter for out-of-order streaming support
                                cfg!(feature = "islands-router"),
                            )
                            .await,
                        )
                    } else {
                        None
                    };

                    response.map(|mut req| {
                        req.extend_response(&res_opts);
                        req
                    })
                })
            })
            .await;

        let response = response.unwrap_or_else(|| {
            let body = Bytes::from("404 not found");
            let mut res =
                http::Response::new(crate::response::Body::Sync(body));
            *res.status_mut() = http::StatusCode::NOT_FOUND;
            res.into()
        });

        let headers = response.headers()?;
        let wasi_res = OutgoingResponse::new(headers);

        wasi_res
            .set_status_code(response.0.status().as_u16())
            .expect("invalid http status code was returned");
        let body = wasi_res.body().expect("unable to take response body");
        ResponseOutparam::set(self.res_out, Ok(wasi_res));

        let output_stream = body
            .write()
            .expect("unable to open writable stream on body");
        let mut input_stream = match response.0.into_body() {
            crate::response::Body::Sync(buf) => {
                Box::pin(stream::once(async { Ok(buf) }))
            }
            crate::response::Body::Async(stream) => stream,
        };

        while let Some(buf) = input_stream.next().await {
            let buf = buf.map_err(HandlerError::ResponseStream)?;
            let chunks = buf.chunks(CHUNK_BYTE_SIZE);
            for chunk in chunks {
                output_stream
                    .blocking_write_and_flush(chunk)
                    .map_err(HandlerError::from)?;
            }
        }

        drop(output_stream);
        OutgoingBody::finish(body, None)
            .map_err(HandlerError::WasiResponseBody)?;

        Ok(())
    }
}

/// A helper trait defining server function implementations that have associated request and response body types.
pub trait ServerWithBody<Error, InputStreamError, OutputStreamError>:
    server_fn::server::Server<
        Error,
        InputStreamError,
        OutputStreamError,
        Request = Request<Self::ReqBody>,
        Response = http::Response<Self::ResBody>,
    >
{
    type ReqBody;
    type ResBody;
}

impl<S, Error, InputStreamError, OutputStreamError, ReqBody, ResBody>
    ServerWithBody<Error, InputStreamError, OutputStreamError> for S
where
    S: server_fn::server::Server<
            Error,
            InputStreamError,
            OutputStreamError,
            Request = Request<ReqBody>,
            Response = http::Response<ResBody>,
        >,
{
    type ReqBody = ReqBody;
    type ResBody = ResBody;
}

type ReqBody<T> = <<T as ServerFn>::Server as ServerWithBody<
    <T as ServerFn>::Error,
    <T as ServerFn>::InputStreamError,
    <T as ServerFn>::OutputStreamError,
>>::ReqBody;

type ResBody<T> = <<T as ServerFn>::Server as ServerWithBody<
    <T as ServerFn>::Error,
    <T as ServerFn>::InputStreamError,
    <T as ServerFn>::OutputStreamError,
>>::ResBody;

fn provide_contexts(
    additional_context: impl Fn() + 'static + Clone + Send,
    context_parts: Parts,
    res_opts: ResponseOptions,
) {
    provide_context(RequestUrl::new(context_parts.uri.path()));
    provide_context(context_parts);
    provide_context(res_opts);
    additional_context();
    provide_server_redirect(redirect);
    leptos::nonce::provide_nonce();
}

trait IntoRouteListing: Sized {
    fn into_route_listing(self) -> Vec<(String, RouteListing)>;
}

impl IntoRouteListing for RouteListing {
    fn into_route_listing(self) -> Vec<(String, RouteListing)> {
        self.path()
            .to_vec()
            .expand_optionals()
            .into_iter()
            .map(|path| {
                let path = path.to_rf_str_representation();
                let path = if path.is_empty() {
                    "/".to_string()
                } else {
                    path
                };
                (path, self.clone())
            })
            .collect()
    }
}

trait RouterPathRepresentation {
    fn to_rf_str_representation(&self) -> String;
}

impl RouterPathRepresentation for Vec<PathSegment> {
    fn to_rf_str_representation(&self) -> String {
        let mut path = String::new();
        for segment in self.iter() {
            // TODO trailing slash handling
            let raw = segment.as_raw_str();
            if !raw.is_empty() && !raw.starts_with('/') {
                path.push('/');
            }
            match segment {
                PathSegment::Static(s) => path.push_str(s),
                PathSegment::Param(s) => {
                    path.push(':');
                    path.push_str(s);
                }
                PathSegment::Splat(_) => {
                    path.push('*');
                }
                PathSegment::Unit => {}
                PathSegment::OptionalParam(_) => {
                    eprintln!(
                        "to_rf_str_representation should only be called on \
                         expanded paths, which do not have OptionalParam any \
                         longer"
                    );
                    Default::default()
                }
            }
        }
        path
    }
}

#[cfg(all(feature = "wasip2", not(feature = "wasip3")))]
/// Errors that can occur during request parsing, route generation, or response streaming.
#[derive(Error, Debug)]
pub enum HandlerError {
    #[error("error handling request")]
    Request(#[from] crate::request::RequestError),

    #[error("error handling response")]
    Response(#[from] crate::response::ResponseError),

    #[error("response stream emitted an error")]
    ResponseStream(throw_error::Error),

    #[error("wasi stream failure")]
    WasiStream(#[from] wasi::io::streams::StreamError),

    #[error("failed to finish response body")]
    WasiResponseBody(wasi::http::types::ErrorCode),
}

#[cfg(feature = "wasip3")]
/// Handles routing, static file serving, and response transmission using WASI Preview 3 HTTP APIs.
///
/// Under WASIp3, incoming requests are represented as standard `http::Request` containing WASIp3 compatibility bodies,
/// and responses are returned directly to the caller.
///
/// # Examples
///
/// ```ignore
/// use leptos::prelude::get_configuration;
/// use leptos_wasi::executor::init_wasip3_spawner;
/// use leptos_wasi::prelude::Handler;
/// use wasip3::http::types::{Request, Response, ErrorCode};
///
/// struct MyServer;
///
/// impl wasip3::exports::http::handler::Guest for MyServer {
///     async fn handle(request: Request) -> Result<Response, ErrorCode> {
///         let _ = init_wasip3_spawner();
///         let conf = get_configuration(None).unwrap();
///         let opt = conf.leptos_options;
///
///         let req = wasip3::http_compat::http_from_wasi_request(request)?;
///
///         let wasi_res = Handler::build(req).await
///             .map_err(|_| ErrorCode::InternalError(None))?
///             .generate_routes(App)
///             .handle_with_context(move || shell(opt.clone()), || {})
///             .await
///             .map_err(|_| ErrorCode::InternalError(None))?;
///
///         Ok(wasi_res)
///     }
/// }
/// ```
#[cfg(feature = "wasip3")]
pub struct Handler {
    req: Request<Bytes>,

    // *shortcut* if any is set
    server_fn: Option<ServerFnHandler>,
    preset_res: Option<Response>,
    should_404: bool,

    // built using the user-defined app_fn
    ssr_router: Router<RouteListing>,
}

#[cfg(feature = "wasip3")]
impl Handler {
    /// Builds a new `Handler` from a compatible WASIp3 HTTP request.
    ///
    /// This asynchronously reads and collects the request body.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let req = wasip3::http_compat::http_from_wasi_request(wasi_req)?;
    /// let handler = Handler::build(req).await?;
    /// ```
    pub async fn build(
        req: Request<wasip3::http_compat::IncomingRequestBody>,
    ) -> Result<Self, HandlerError> {
        let (parts, body) = req.into_parts();

        use http_body_util::{BodyExt, Limited};
        let limited_body = Limited::new(body, MAX_REQUEST_BODY_SIZE);

        match limited_body.collect().await {
            Ok(collected) => {
                let http_req = Request::from_parts(parts, collected.to_bytes());
                Ok(Self {
                    req: http_req,
                    server_fn: None,
                    preset_res: None,
                    ssr_router: Router::new(),
                    should_404: false,
                })
            }
            Err(e) => {
                if e.is::<http_body_util::LengthLimitError>() {
                    let error_msg = format!(
                        "Request body too large (max: {} bytes)",
                        MAX_REQUEST_BODY_SIZE
                    );
                    let mut res = http::Response::new(
                        crate::response::Body::Sync(Bytes::from(error_msg)),
                    );
                    *res.status_mut() = StatusCode::PAYLOAD_TOO_LARGE;
                    Ok(Self {
                        req: Request::new(Bytes::new()),
                        server_fn: None,
                        preset_res: Some(res.into()),
                        ssr_router: Router::new(),
                        should_404: false,
                    })
                } else if let Ok(wasi_err) =
                    e.downcast::<wasip3::http::types::ErrorCode>()
                {
                    Err(HandlerError::Request(
                        crate::request::RequestError::Wasi(*wasi_err),
                    ))
                } else {
                    Err(HandlerError::Request(
                        crate::request::RequestError::Wasi(
                            wasip3::http::types::ErrorCode::InternalError(None),
                        ),
                    ))
                }
            }
        }
    }

    #[inline]
    const fn shortcut(&self) -> bool {
        self.server_fn.is_some() || self.preset_res.is_some() || self.should_404
    }

    /// Tests if the request path matches the bound server function and shortcuts
    /// the [`Handler`] to quickly serve the response.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let handler = handler.with_server_fn::<MyServerFn>();
    /// ```
    pub fn with_server_fn<T>(mut self) -> Self
    where
        T: ServerFn + 'static,
        T::Server:
            ServerWithBody<T::Error, T::InputStreamError, T::OutputStreamError>,
        ReqBody<T>: Into<crate::response::Body> + From<Bytes> + 'static,
        ResBody<T>: Into<crate::response::Body> + 'static,
    {
        if self.shortcut() {
            return self;
        }

        if self.req.method()
            == <T::Protocol as Protocol<
                T,
                T::Output,
                T::Client,
                T::Server,
                T::Error,
                T::InputStreamError,
                T::OutputStreamError,
            >>::METHOD
            && self.req.uri().path() == T::PATH
        {
            self.server_fn = Some(Box::new(move |request| {
                Box::pin(async move {
                    let (parts, body) = request.into_parts();
                    let server_body = match body {
                        Body::Sync(bytes) => {
                            if bytes.len() > MAX_REQUEST_BODY_SIZE {
                                let error_msg = format!(
                                    "Request body too large (max: {} bytes)",
                                    MAX_REQUEST_BODY_SIZE
                                );
                                let error_response = http::Response::builder()
                                    .status(413) // Payload Too Large
                                    .body(Body::Sync(Bytes::from(error_msg)))
                                    .unwrap();
                                return error_response;
                            }
                            ReqBody::<T>::from(bytes)
                        }
                        Body::Async(mut stream) => {
                            use futures::StreamExt;
                            let mut collected_bytes = Vec::new();

                            while let Some(chunk_result) = stream.next().await {
                                match chunk_result {
                                    Ok(chunk) => {
                                        if collected_bytes.len() + chunk.len()
                                            > MAX_REQUEST_BODY_SIZE
                                        {
                                            let error_msg = format!(
                                                "Request body too large (max: \
                                                 {} bytes)",
                                                MAX_REQUEST_BODY_SIZE
                                            );
                                            let error_response =
                                                http::Response::builder()
                                                    .status(413)
                                                    .body(Body::Sync(
                                                        Bytes::from(error_msg),
                                                    ))
                                                    .unwrap();
                                            return error_response;
                                        }
                                        collected_bytes
                                            .extend_from_slice(&chunk);
                                    }
                                    Err(e) => {
                                        let error_msg = format!(
                                            "Failed to read request body: {}",
                                            e
                                        );
                                        let error_response =
                                            http::Response::builder()
                                                .status(400)
                                                .body(Body::Sync(Bytes::from(
                                                    error_msg,
                                                )))
                                                .unwrap();
                                        return error_response;
                                    }
                                }
                            }

                            ReqBody::<T>::from(Bytes::from(collected_bytes))
                        }
                    };

                    let server_request =
                        Request::from_parts(parts, server_body);
                    let response = T::run_on_server(server_request).await;
                    response.map(|body| {
                        let our_body: crate::response::Body = body.into();
                        match our_body {
                            crate::response::Body::Sync(bytes) => {
                                Body::Sync(bytes)
                            }
                            crate::response::Body::Async(stream) => {
                                Body::Async(stream)
                            }
                        }
                    })
                })
            }));
        }

        self
    }

    pub fn with_server_fn_generic<T>(self) -> Self
    where
        T: ServerFn + 'static,
        T::Server:
            ServerWithBody<T::Error, T::InputStreamError, T::OutputStreamError>,
        ReqBody<T>: Into<crate::response::Body> + From<Bytes> + 'static,
        ResBody<T>: Into<crate::response::Body> + 'static,
    {
        self.with_server_fn::<T>()
    }

    pub fn with_server_fn_axum<T>(self) -> Self
    where
        T: ServerFn + 'static,
        T::Server: ServerWithBody<
                T::Error,
                T::InputStreamError,
                T::OutputStreamError,
                ReqBody = axum_core::body::Body,
                ResBody = axum_core::body::Body,
            >,
    {
        self.with_server_fn::<T>()
    }

    /// Registers a custom static file handler for a specific URI prefix.
    ///
    /// If the request URL starts with the prefix, the callback is executed to resolve the file.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let handler = handler.static_files_handler("/assets", |path| {
    ///     let file_bytes = load_from_blobstore(&path)?;
    ///     Some(leptos_wasi::response::Body::Sync(file_bytes))
    /// });
    /// ```
    pub fn static_files_handler<T>(
        mut self,
        prefix: T,
        handler: impl Fn(String) -> Option<crate::response::Body>
        + 'static
        + Send
        + Clone,
    ) -> Self
    where
        T: TryInto<Uri>,
        <T as TryInto<Uri>>::Error: std::error::Error,
    {
        if self.shortcut() {
            return self;
        }

        let req_path = self.req.uri().path();
        let prefix_uri = prefix.try_into().expect("you passed an invalid Uri");
        let prefix_path = prefix_uri.path();

        let is_match = if req_path == prefix_path {
            true
        } else if let Some(rest) = req_path.strip_prefix(prefix_path) {
            rest.starts_with('/') || prefix_path.ends_with('/')
        } else {
            false
        };

        if is_match {
            let stripped_url = req_path.strip_prefix(prefix_path).unwrap_or("");
            let trimmed_url = stripped_url.trim_start_matches('/');
            let decoded_url = url_decode(trimmed_url);

            // Security: reject path traversal attempts before
            // invoking the user-provided file handler.
            if decoded_url.contains("..") || decoded_url.contains('\\') {
                self.should_404 = true;
                return self;
            }

            match handler(decoded_url.clone()) {
                None => self.should_404 = true,
                Some(body) => {
                    let mut res = http::Response::new(body);
                    let mime = MimeGuess::from_path(&decoded_url);

                    res.headers_mut().insert(
                        http::header::CONTENT_TYPE,
                        HeaderValue::from_str(
                            mime.first_or_octet_stream().as_ref(),
                        )
                        .expect("internal error: could not parse MIME type"),
                    );

                    self.preset_res = Some(res.into());
                }
            }
        }

        self
    }

    /// Generates routes for the application from the root component.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let handler = handler.generate_routes(App);
    /// ```
    pub fn generate_routes<IV>(
        self,
        app_fn: impl Fn() -> IV + 'static + Send + Clone,
    ) -> Self
    where
        IV: IntoView + 'static,
    {
        self.generate_routes_with_exclusions_and_context(app_fn, None, || {})
    }

    /// Generates routes for the application and injects custom contexts.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let handler = handler.generate_routes_with_context(App, || {
    ///     provide_context(MyGlobalState::new());
    /// });
    /// ```
    pub fn generate_routes_with_context<IV>(
        self,
        app_fn: impl Fn() -> IV + 'static + Send + Clone,
        additional_context: impl Fn() + 'static + Send + Clone,
    ) -> Self
    where
        IV: IntoView + 'static,
    {
        self.generate_routes_with_exclusions_and_context(
            app_fn,
            None,
            additional_context,
        )
    }

    /// Generates routes for the application, excluding specific paths and injecting custom contexts.
    pub fn generate_routes_with_exclusions_and_context<IV>(
        mut self,
        app_fn: impl Fn() -> IV + 'static + Send + Clone,
        excluded_routes: Option<Vec<String>>,
        additional_context: impl Fn() + 'static + Send + Clone,
    ) -> Self
    where
        IV: IntoView + 'static,
    {
        if self.shortcut() {
            return self;
        }

        if !self.ssr_router.is_empty() {
            panic!("generate_routes was called twice");
        }

        let owner = Owner::new_root(Some(Arc::new(SsrSharedContext::new())));
        let routes = owner
            .with(|| {
                provide_context(RequestUrl::new(""));
                let (mock_meta, _) = ServerMetaContext::new();
                let (mock_parts, _) = Request::new("").into_parts();
                provide_context(mock_meta);
                provide_context(mock_parts);
                provide_context(ResponseOptions::default());
                additional_context();
                RouteList::generate(&app_fn)
            })
            .unwrap_or_default()
            .into_inner()
            .into_iter()
            .flat_map(IntoRouteListing::into_route_listing)
            .filter(|route| {
                excluded_routes.as_ref().is_none_or(|excluded_routes| {
                    !excluded_routes.contains(&route.0)
                })
            });

        for (path, route_listing) in routes {
            self.ssr_router
                .add(path, route_listing)
                .expect("internal error: impossible to parse a RouteListing");
        }

        self
    }

    /// Consumes the [`Handler`] to execute routing, SSR rendering, and returns the compiled
    /// WASIp3 HTTP `Response`.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let response = handler.handle_with_context(
    ///     move || shell(leptos_options.clone()),
    ///     || { provide_context(db_connection.clone()); }
    /// ).await?;
    /// ```
    pub async fn handle_with_context<IV>(
        self,
        app: impl Fn() -> IV + 'static + Send + Clone,
        additional_context: impl Fn() + 'static + Clone + Send,
        // Wait, under WASIp3 the output is Result<wasip3::http::types::Response, HandlerError>
        // Let's verify: yes, let's keep the return type exactly as is.
    ) -> Result<wasip3::http::types::Response, HandlerError>
    where
        IV: IntoView + 'static,
    {
        let path = self.req.uri().path().to_string();
        let best_match = self.ssr_router.best_match(&path);
        let (parts, body) = self.req.into_parts();
        let context_parts = parts.clone();
        let req = Request::from_parts(parts, body);

        let owner = Owner::new();
        let response = owner
            .with(|| {
                ScopedFuture::new(async move {
                    let res_opts = ResponseOptions::default();
                    let response: Option<Response> = if self.should_404 {
                        None
                    } else if self.preset_res.is_some() {
                        self.preset_res
                    } else if let Some(sfn) = self.server_fn {
                        provide_contexts(additional_context, context_parts, res_opts.clone());

                        let accepts_html = req
                            .headers()
                            .get(ACCEPT)
                            .and_then(|v| v.to_str().ok())
                            .map(|v| v.contains("text/html"))
                            .unwrap_or(false);
                        let referrer = req
                            .headers()
                            .get(REFERER)
                            .or_else(|| req.headers().get("referrer"))
                            .cloned();

                        let req_with_body = req.map(Body::from);
                        let mut res = sfn(req_with_body).await;

                        let mut redirect_target = None;

                        if let (true, Some(referrer)) = (accepts_html, referrer.clone()) {
                            let is_default_redirect = res.headers().get(LOCATION)
                                .and_then(|v| v.to_str().ok())
                                == Some("/");
                            let has_location = res.headers().get(LOCATION).is_some();
                            if !has_location || is_default_redirect {
                                if let Some(sanitized) = sanitize_referrer(&referrer) {
                                    *res.status_mut() = StatusCode::FOUND;
                                    redirect_target = Some(sanitized);
                                } else if !has_location {
                                    *res.status_mut() = StatusCode::FOUND;
                                    redirect_target = Some(HeaderValue::from_static("/"));
                                }
                            }
                        }

                        if let (None, Some(location)) = (redirect_target.as_ref(), res.headers().get(LOCATION).cloned()) {
                            let sanitized = sanitize_referrer(&location)
                                .unwrap_or_else(|| HeaderValue::from_static("/"));
                            redirect_target = Some(sanitized);
                        }

                        if let Some(target) = redirect_target {
                            res.headers_mut().insert(LOCATION, target);
                        }

                        Some(res.into())
                    } else if let Some(best_match) = best_match {
                        let listing = best_match.handler();
                        let (meta_context, meta_output) = ServerMetaContext::new();

                        let add_ctx = additional_context.clone();
                        let additional_context = {
                            let res_opts = res_opts.clone();
                            let meta_ctx = meta_context.clone();
                            move || {
                                provide_contexts(add_ctx, context_parts, res_opts);
                                provide_context(meta_ctx);
                            }
                        };

                        Some(
                            Response::from_app(
                                app,
                                meta_output,
                                additional_context,
                                res_opts.clone(),
                                match listing.mode() {
                                    SsrMode::Async => |app, chunks, _| {
                                        Box::pin(async move {
                                            let app = if cfg!(feature = "islands-router") {
                                                app.to_html_stream_in_order_branching()
                                            } else {
                                                app.to_html_stream_in_order()
                                            };
                                            let app = app.collect::<String>().await;
                                            let chunks = chunks();
                                            Box::pin(once(async move { app }).chain(chunks))
                                                as PinnedStream<String>
                                        })
                                    },
                                    SsrMode::InOrder => |app, chunks, _| {
                                        Box::pin(async move {
                                            let app = if cfg!(feature = "islands-router") {
                                                app.to_html_stream_in_order_branching()
                                            } else {
                                                app.to_html_stream_in_order()
                                            };
                                            Box::pin(app.chain(chunks())) as PinnedStream<String>
                                        })
                                    },
                                    SsrMode::PartiallyBlocked | SsrMode::OutOfOrder => {
                                        |app, chunks, _| {
                                            Box::pin(async move {
                                                let app = if cfg!(feature = "islands-router") {
                                                    app.to_html_stream_out_of_order_branching()
                                                } else {
                                                    app.to_html_stream_out_of_order()
                                                };
                                                Box::pin(app.chain(chunks()))
                                                    as PinnedStream<String>
                                            })
                                        }
                                    }
                                    SsrMode::Static(_) => {
                                        panic!("SsrMode::Static routes are not supported yet!")
                                    }
                                },
                                cfg!(feature = "islands-router"),
                            )
                            .await,
                        )
                    } else {
                        None
                    };

                    response.map(|mut req| {
                        req.extend_response(&res_opts);
                        req
                    })
                })
            })
            .await;

        let response = response.unwrap_or_else(|| {
            let body = Bytes::from("404 not found");
            let mut res =
                http::Response::new(crate::response::Body::Sync(body));
            *res.status_mut() = http::StatusCode::NOT_FOUND;
            res.into()
        });

        let mapped_response = response.0.map(|body| {
            use http_body_util::BodyExt;
            body.map_frame(|frame| {
                frame.map_data(|bytes| WasiBuf(bytes.to_vec()))
            })
            .map_err(|e| std::io::Error::other(e.to_string()))
        });

        let wasi_res =
            wasip3::http_compat::http_into_wasi_response(mapped_response)
                .map_err(HandlerError::Wasi)?;
        Ok(wasi_res)
    }
}

#[cfg(feature = "wasip3")]
/// A buffer wrapper around a `Vec<u8>` that implements the `bytes::Buf` trait.
/// Used for translating between guest buffers and host-compatible HTTP body structures.
#[derive(Clone, Debug)]
pub struct WasiBuf(pub Vec<u8>);

#[cfg(feature = "wasip3")]
impl bytes::Buf for WasiBuf {
    fn remaining(&self) -> usize {
        self.0.len()
    }

    fn chunk(&self) -> &[u8] {
        &self.0
    }

    fn advance(&mut self, cnt: usize) {
        self.0.drain(..cnt);
    }
}

#[cfg(feature = "wasip3")]
impl From<WasiBuf> for Vec<u8> {
    fn from(buf: WasiBuf) -> Self {
        buf.0
    }
}

#[cfg(feature = "wasip3")]
/// Errors that can occur during request parsing, route generation, or response streaming.
#[derive(Error, Debug)]
pub enum HandlerError {
    #[error("error handling request")]
    Request(#[from] crate::request::RequestError),

    #[error("response stream emitted an error")]
    ResponseStream(throw_error::Error),

    #[error("wasi http error: {0:?}")]
    Wasi(wasip3::http::types::ErrorCode),
}

fn url_decode(s: &str) -> String {
    let mut decoded = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let decoded_byte = if bytes[i] == b'%' && i + 2 < bytes.len() {
            std::str::from_utf8(&bytes[i + 1..i + 3])
                .ok()
                .and_then(|hex_str| u8::from_str_radix(hex_str, 16).ok())
        } else {
            None
        };

        if let Some(val) = decoded_byte {
            decoded.push(val);
            i += 3;
            continue;
        }
        decoded.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(decoded).unwrap_or_else(|_| s.to_string())
}

fn sanitize_referrer(referrer: &HeaderValue) -> Option<HeaderValue> {
    let referrer_str = referrer.to_str().ok()?;
    let uri = referrer_str.parse::<http::Uri>().ok()?;
    let pq = uri.path_and_query()?;
    let pq_str = pq.as_str();
    if pq_str.starts_with("/\\")
        || pq_str.contains('\\')
        || pq_str.contains("%5c")
        || pq_str.contains("%5C")
    {
        return None;
    }
    if pq_str.starts_with('/') && !pq_str.starts_with("//") {
        HeaderValue::from_str(pq_str).ok()
    } else {
        None
    }
}
