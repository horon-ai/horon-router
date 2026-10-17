//! Credential-level quota routes (plugin_quota.go `FetchCredentialQuota` and
//! `ResetCredentialQuota`): the v0 spellings and the v8 `quota/fetch` the dashboard
//! uses.
//!
//! The plugin quota providers are gone with the plugin host, so both handlers now see an
//! empty provider registry: `fetch` falls back to the credential's declarative
//! `quota_probe` (management/quota_probe.rs) and the command-code endpoint, and `reset`
//! answers what an empty registry answered before (501 for a credential, 404 for a
//! plugin-named request).

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use horon_common::gojson::to_vec;
use horon_core::config::credentials;
use horon_core::credential::Credential;
use horon_exec::command_code::DEFAULT_BASE_URL;
use serde_json::{Value, json};

use super::Management;
use super::quota_types::QuotaBody;

fn map_json(status: StatusCode, value: &Value) -> Response {
    go_json(status, crate::gojson::sorted(value).into_bytes())
}

fn go_json(status: StatusCode, body: Vec<u8>) -> Response {
    (
        status,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        )],
        body,
    )
        .into_response()
}

fn fail(status: StatusCode, message: &str) -> Response {
    map_json(status, &json!({"error": message}))
}

fn credential(state: &Management, index: &str) -> Option<Arc<Credential>> {
    state
        .rt
        .store()
        .snapshot()
        .into_iter()
        .find(|c| credentials::auth_index(c) == index)
}

/// `POST /v0/management/quota/fetch` (Go `FetchCredentialQuota`).
pub(crate) async fn fetch(State(state): State<Arc<Management>>, body: Bytes) -> Response {
    let Some(body) = QuotaBody::decode(&body) else {
        return fail(StatusCode::BAD_REQUEST, "invalid request body");
    };
    let index = body.auth_index();
    if index.is_empty() {
        return fail(StatusCode::BAD_REQUEST, "auth_index is required");
    }
    let Some(auth) = credential(&state, &index) else {
        return fail(StatusCode::NOT_FOUND, "auth not found");
    };
    // Go: the declarative quota probe, when the metadata declares one.
    if let Some(probe) = auth.metadata.get("quota_probe").and_then(Value::as_object) {
        match super::quota_probe::execute(&state, &auth, probe).await {
            Some(Ok(resp)) => return go_json(StatusCode::OK, to_vec(&resp)),
            Some(Err(e)) => return fail(StatusCode::BAD_GATEWAY, &format!("quota probe failed: {e}")),
            None => {}
        }
    }
    if auth.provider == "command-code" {
        match command_code_quota(&state, &auth).await {
            Some(Ok(body)) => return go_json(StatusCode::OK, body),
            Some(Err(e)) => return fail(StatusCode::BAD_GATEWAY, &format!("failed to fetch quota: {e}")),
            None => {}
        }
    }
    fail(
        StatusCode::NOT_IMPLEMENTED,
        "no quota provider available for credential",
    )
}

async fn command_code_quota(state: &Management, auth: &Credential) -> Option<Result<Vec<u8>, String>> {
    let key = super::api_call::token_for(auth);
    if key.is_empty() {
        return Some(Err("auth token not found".into()));
    }
    let base = ["base_url"]
        .into_iter()
        .find_map(|k| {
            auth.attributes
                .get(k)
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .or_else(|| auth.str(k).map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()))
        })
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned());
    let client = super::api_call::client_for(state, Some(auth), "")?;
    match horon_exec::command_code_quota::fetch(&client, &base, &key).await {
        Some(value) => Some(Ok(value.to_string().into_bytes())),
        None => Some(Err("command-code quota endpoints returned no usable body".into())),
    }
}

/// `POST /v0/management/quota/reset` (Go `ResetCredentialQuota`). The reset itself ran
/// through a plugin quota provider, so a request that names one and a credential-level
/// one are answered the way an empty provider registry answered them (Go
/// `plugin_quota.go`).
pub(crate) async fn reset(State(state): State<Arc<Management>>, body: Bytes) -> Response {
    let Some(body) = QuotaBody::decode(&body) else {
        return fail(StatusCode::BAD_REQUEST, "invalid request body");
    };
    let index = body.auth_index();
    if index.is_empty() {
        return fail(StatusCode::BAD_REQUEST, "auth_index is required");
    }
    if credential(&state, &index).is_none() {
        return fail(StatusCode::NOT_FOUND, "auth not found");
    }
    if !body.plugin_id.trim().is_empty() {
        return fail(StatusCode::NOT_FOUND, "quota provider not found for plugin");
    }
    fail(
        StatusCode::NOT_IMPLEMENTED,
        "no quota provider available for credential to reset",
    )
}
