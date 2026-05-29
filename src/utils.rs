use crate::response::ResponseOptions;
use http::{HeaderName, HeaderValue, StatusCode, header, request::Parts};
use leptos::prelude::use_context;
use server_fn::redirect::REDIRECT_HEADER;

/// Allows returning an HTTP redirection from components.
///
/// Inspects the current Leptos context for `Parts` and `ResponseOptions` to insert the
/// `Location` header or set a 302 status code.
///
/// # Example
///
/// ```ignore
/// use leptos_wasi::utils::redirect;
/// use leptos::prelude::*;
///
/// #[component]
/// fn RedirectButton() -> impl IntoView {
///     let on_click = |_| {
///         redirect("/target-page");
///     };
///     view! { <button on:click=on_click>"Go"</button> }
/// }
/// ```
pub fn redirect(path: &str) {
    if let (Some(req), Some(res)) =
        (use_context::<Parts>(), use_context::<ResponseOptions>())
    {
        // insert the Location header in any case
        match header::HeaderValue::from_str(path) {
            Ok(value) => {
                res.insert_header(header::LOCATION, value);
            }
            Err(e) => {
                eprintln!("Invalid redirect path: {}, error: {:?}", path, e);
                res.set_status(StatusCode::BAD_REQUEST);
                return;
            }
        }

        let accepts_html = req
            .headers
            .get(header::ACCEPT)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.contains("text/html"))
            .unwrap_or(false);
        if accepts_html {
            // if the request accepts text/html, it's a plain form request and needs
            // to have the 302 code set
            res.set_status(StatusCode::FOUND);
        } else {
            // otherwise, we sent it from the server fn client and actually don't want
            // to set a real redirect, as this will break the ability to return data
            // instead, set the REDIRECT_HEADER to indicate that the client should redirect
            res.insert_header(
                HeaderName::from_static(REDIRECT_HEADER),
                HeaderValue::from_str("").unwrap(),
            );
        }
    } else {
        eprintln!(
            "Couldn't retrieve either Parts or ResponseOptions while trying \
             to redirect()."
        );
    }
}
