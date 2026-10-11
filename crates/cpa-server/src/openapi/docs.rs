//! The local API reference. The Scalar standalone bundle is embedded at build time (see
//! `assets/scalar/PROVENANCE.md`), so the page works offline and never reaches a CDN, a font
//! host or the Scalar proxy; the options in [`DOCS_HTML`] disable agent, proxy, telemetry,
//! fonts, MCP and developer tools, so no key leaves the server through the reference.
//!
//! The routes are public like the health and callback routes, and [`crate::api`] merges them
//! after the client-key layer, so they never weaken the API's own authentication. They are not
//! described by the document they serve, excluded the way the static panel paths are.

use std::sync::Arc;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::http::{HeaderValue, Method, header};
use axum::response::Response;
use axum::routing::get;

use crate::runtime::Runtime;

/// The pinned Scalar standalone bundle; updating the reference means replacing this file and
/// `assets/scalar/PROVENANCE.md`, not this module.
const SCALAR_JS: &[u8] = include_bytes!("../../assets/scalar/scalar-standalone.js");

const DOCS_MIME: &str = "text/html; charset=utf-8";
const SPEC_MIME: &str = "application/json; charset=utf-8";
const SCALAR_JS_MIME: &str = "text/javascript; charset=utf-8";

/// The options were verified against the pinned bundle in
/// `.omo/evidence/server-openapi-scalar/task-6/`; `servers: [{ url: "/" }]` keeps the Test
/// Request on the same origin as the page.
const DOCS_HTML: &str = r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>CLIProxyAPI API Reference</title>
  </head>
  <body>
    <div id="app"></div>
    <script src="/docs/assets/scalar.js"></script>
    <script>
      Scalar.createApiReference('#app', {
        url: '/openapi.json',
        servers: [{ url: '/' }],
        persistAuth: false,
        telemetry: false,
        withDefaultFonts: false,
        agent: { disabled: true },
        showDeveloperTools: 'never',
        proxyUrl: '',
        mcp: { disabled: true }
      });
    </script>
  </body>
</html>
"#;

pub fn routes() -> Router<Arc<Runtime>> {
    Router::new()
        .route("/docs", get(docs).head(docs))
        .route("/docs/assets/scalar.js", get(scalar_js).head(scalar_js))
        .route("/openapi.json", get(spec).head(spec))
}

async fn docs(method: Method) -> Response {
    respond(method, DOCS_MIME, Bytes::from_static(DOCS_HTML.as_bytes()))
}

async fn scalar_js(method: Method) -> Response {
    respond(method, SCALAR_JS_MIME, Bytes::from_static(SCALAR_JS))
}

async fn spec(method: Method) -> Response {
    let body = serde_json::to_vec(&super::document()).expect("a serde_json::Value always serializes");
    respond(method, SPEC_MIME, Bytes::from(body))
}

/// `no-cache` is deliberate: the spec tracks the running binary's version and the bundle
/// tracks the pinned release, so a cached copy must always be revalidated (there is no
/// `ETag` yet, so revalidation re-fetches; add a content hash if that becomes a cost).
/// `HEAD` carries the `GET` headers, including the `GET` length, with an empty body.
fn respond(method: Method, content_type: &'static str, body: Bytes) -> Response {
    let mut response = if method == Method::HEAD {
        let mut response = Response::new(Body::empty());
        let length = HeaderValue::from_str(&body.len().to_string()).expect("a byte count is a valid header value");
        response.headers_mut().insert(header::CONTENT_LENGTH, length);
        response
    } else {
        Response::new(Body::from(body))
    };
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    response
}
