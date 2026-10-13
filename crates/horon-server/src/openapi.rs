//! OpenAPI 3.1 document for the fixed HTTP API: the general routes registered in
//! [`crate::router`] and [`crate::app`], plus the management routes merged in from
//! [`management`]. This module only *describes* routes; it does not register them, touch
//! handlers, or alter the request path. The dynamic plugin routes are still out of scope.
//!
//! The document is a static [`serde_json::Value`] built at call time. Provider payloads
//! are pass-through, so request and response schemas allow extra properties
//! (`additionalProperties: true`) instead of pinning provider-specific fields.

pub(crate) mod docs;
mod management;

use serde_json::{Map, Value, json};

/// The OpenAPI version this document conforms to.
pub const OPENAPI_VERSION: &str = "3.1.0";

const JSON: &str = "application/json";
const SSE: &str = "text/event-stream";
const MULTIPART: &str = "multipart/form-data";
const FORM: &str = "application/x-www-form-urlencoded";
const SDP: &str = "application/sdp";
const OCTET: &str = "application/octet-stream";
const TEXT: &str = "text/plain";
const HTML: &str = "text/html";

const TAG_GENERAL: &str = "General";
const TAG_CALLBACK: &str = "Callback";
const TAG_OPENAI: &str = "OpenAI";
const TAG_CLAUDE: &str = "Claude";
const TAG_GEMINI: &str = "Gemini";
const TAG_IMAGES: &str = "Images";
const TAG_VIDEOS: &str = "Videos";
const TAG_CODEX: &str = "Codex";
const TAG_REALTIME: &str = "Realtime";
const TAG_WEBSOCKET: &str = "WebSocket";

/// Builds the whole OpenAPI 3.1 document: the general API and the merged management API.
pub fn document() -> Value {
    let mut document = json!({
        "openapi": OPENAPI_VERSION,
        "info": {
            "title": "CLIProxyAPI",
            "version": env!("CARGO_PKG_VERSION"),
            "description": "Fixed HTTP API of the local AI proxy. Client requests \
    are forwarded to the selected upstream provider. General authentication uses the \
    configured client keys: the Authorization header (a `Bearer <key>` value or the raw \
    key), the `X-Goog-Api-Key` and `X-Api-Key` headers, or the `key` and `auth_token` \
    query values; the five are alternatives (OR). When no keys are configured the API is \
    open. The realtime call and sideband routes additionally accept an ephemeral `ek_...` \
    client secret. The management API under `/v8/management` (and its deprecated \
    `/v0/management` spellings) is included and uses its own management key, separate from \
    the client keys. Dynamic plugin routes, WebSocket message payloads and the RESP \
    protocol are not part of this document.",
        },
        "servers": [ { "url": "/" } ],
        "tags": tags(),
        "paths": paths(),
        "components": components(),
    });
    management::extend(&mut document);
    document
}

fn tags() -> Value {
    json!([
        { "name": TAG_GENERAL, "description": "Health and root endpoints." },
        { "name": TAG_CALLBACK, "description": "Provider OAuth redirect callbacks (public)." },
        { "name": TAG_OPENAI, "description": "OpenAI Chat Completions, Completions and Responses." },
        { "name": TAG_CLAUDE, "description": "Anthropic Messages." },
        { "name": TAG_GEMINI, "description": "Gemini generateContent and Interactions." },
        { "name": TAG_IMAGES, "description": "Image generation and edits." },
        { "name": TAG_VIDEOS, "description": "Video generation, retrieval and content." },
        { "name": TAG_CODEX, "description": "Codex Alpha Search." },
        { "name": TAG_REALTIME, "description": "Realtime / Live call negotiation, ephemeral secrets and hangup." },
        { "name": TAG_WEBSOCKET, "description": "WebSocket upgrade endpoints." },
    ])
}

fn r(name: &str) -> Value {
    json!({ "$ref": format!("#/components/schemas/{name}") })
}

/// A realtime call request: the SDP offer as a multipart `sdp` field, a raw
/// `application/sdp` or `text/plain` body, or a JSON body carrying `sdp`/`session`/`model`.
fn live_call_request_content() -> Value {
    body_content(&[
        (
            MULTIPART,
            json!({
                "type": "object",
                "description": "Multipart form with the SDP offer and an optional session object.",
                "properties": {
                    "sdp": { "type": "string", "description": "The WebRTC SDP offer." },
                    "session": { "type": "object", "additionalProperties": true }
                },
                "required": ["sdp"]
            }),
        ),
        (
            SDP,
            json!({ "type": "string", "description": "The WebRTC SDP offer as the raw body." }),
        ),
        (
            TEXT,
            json!({ "type": "string", "description": "The WebRTC SDP offer as the raw body (text/plain)." }),
        ),
        (JSON, r("JsonBody")),
    ])
}

/// The `/v1/models` body: one of the three shapes `models.rs` selects by request headers.
fn model_list_schema() -> Value {
    json!({ "oneOf": [ r("ModelList"), r("ClaudeModelList"), r("CodexModelCatalog") ] })
}

/// Configured client keys plus an anonymous alternative (`{}`), for routes whose client-key
/// check is conditional at runtime.
fn optional_client_security() -> Value {
    json!([
        { "clientBearer": [] },
        { "clientGoogApiKey": [] },
        { "clientApiKey": [] },
        { "clientQueryKey": [] },
        { "clientQueryAuthToken": [] },
        {}
    ])
}

fn body_content(entries: &[(&str, Value)]) -> Value {
    let mut m = Map::new();
    for (media_type, schema) in entries {
        m.insert((*media_type).into(), json!({ "schema": schema.clone() }));
    }
    Value::Object(m)
}

fn resp(description: &str, entries: &[(&str, Value)]) -> Value {
    if entries.is_empty() {
        return json!({ "description": description });
    }
    json!({ "description": description, "content": body_content(entries) })
}

fn path_param(name: &str, description: &str) -> Value {
    json!({
        "name": name,
        "in": "path",
        "required": true,
        "schema": { "type": "string" },
        "description": description,
    })
}

fn query_param(name: &str, description: &str) -> Value {
    json!({
        "name": name,
        "in": "query",
        "required": false,
        "schema": { "type": "string" },
        "description": description,
    })
}

/// The standard error statuses every forwarding route can answer with.
fn error_responses(schema: &str) -> Vec<(&'static str, Value)> {
    vec![
        ("400", resp("Invalid request", &[(JSON, r(schema))])),
        ("401", resp("Missing or invalid API key", &[(JSON, r(schema))])),
        ("403", resp("Forbidden", &[(JSON, r(schema))])),
        ("404", resp("Not found", &[(JSON, r(schema))])),
        (
            "429",
            resp("Rate limited or the model is cooling down", &[(JSON, r(schema))]),
        ),
        ("500", resp("Server error", &[(JSON, r(schema))])),
        ("503", resp("No upstream credential is available", &[(JSON, r(schema))])),
    ]
}

fn with_errors(mut ok: Vec<(&'static str, Value)>, schema: &str) -> Vec<(&'static str, Value)> {
    ok.extend(error_responses(schema));
    ok
}

fn op(
    id: &str,
    tag: &str,
    summary: &str,
    security: Value,
    params: Vec<Value>,
    request_body: Option<Value>,
    responses: Vec<(&'static str, Value)>,
) -> Value {
    let mut m = Map::new();
    m.insert("operationId".into(), Value::String(id.into()));
    m.insert("tags".into(), json!([tag]));
    m.insert("summary".into(), Value::String(summary.into()));
    m.insert("security".into(), security);
    if !params.is_empty() {
        m.insert("parameters".into(), Value::Array(params));
    }
    if let Some(content) = request_body {
        m.insert("requestBody".into(), json!({ "required": true, "content": content }));
    }
    let mut rs = Map::new();
    for (status, response) in responses {
        rs.insert(status.into(), response);
    }
    m.insert("responses".into(), Value::Object(rs));
    Value::Object(m)
}

fn path_item(ops: Vec<(&'static str, Value)>) -> Value {
    let mut m = Map::new();
    for (method, operation) in ops {
        m.insert(method.into(), operation);
    }
    Value::Object(m)
}

/// Configured client keys, in every location `access.rs` accepts, as alternatives (OR).
fn client_security() -> Value {
    json!([
        { "clientBearer": [] },
        { "clientGoogApiKey": [] },
        { "clientApiKey": [] },
        { "clientQueryKey": [] },
        { "clientQueryAuthToken": [] }
    ])
}

/// The realtime call and sideband routes also accept an ephemeral `ek_...` client secret.
fn realtime_security() -> Value {
    json!([
        { "clientBearer": [] },
        { "clientGoogApiKey": [] },
        { "clientApiKey": [] },
        { "clientQueryKey": [] },
        { "clientQueryAuthToken": [] },
        { "realtimeClientSecret": [] }
    ])
}

fn public_security() -> Value {
    json!([])
}

fn components() -> Value {
    json!({
        "securitySchemes": {
            "clientBearer": {
                "type": "http",
                "scheme": "bearer",
                "description": "Authorization header: `Bearer <key>` or the raw key."
            },
            "clientGoogApiKey": { "type": "apiKey", "in": "header", "name": "X-Goog-Api-Key" },
            "clientApiKey": { "type": "apiKey", "in": "header", "name": "X-Api-Key" },
            "clientQueryKey": { "type": "apiKey", "in": "query", "name": "key" },
            "clientQueryAuthToken": { "type": "apiKey", "in": "query", "name": "auth_token" },
            "realtimeClientSecret": {
                "type": "http",
                "scheme": "bearer",
                "description": "Ephemeral `ek_...` client secret issued by \
    POST /v1/realtime/client_secrets or POST /v1/realtime/sessions; accepted by the realtime \
    call and sideband routes in place of a configured client key."
            }
        },
        "schemas": schemas()
    })
}

fn schemas() -> Value {
    json!({
        "JsonBody": {
            "type": "object",
            "description": "Provider payload forwarded byte-for-byte. Fields vary by provider and \
    model; this schema documents the common envelope without constraining provider-specific keys.",
            "additionalProperties": true
        },
        "Error": {
            "type": "object",
            "description": "Error envelope for the OpenAI, Responses, Gemini and Interactions routes \
    (`{\"error\": {\"message\", \"type\", \"code\"}}`). A valid-JSON upstream error is passed through \
    unchanged.",
            "properties": {
                "error": {
                    "type": "object",
                    "properties": {
                        "message": { "type": "string" },
                        "type": { "type": "string" },
                        "code": { "type": "string" },
                        "param": { "type": ["string", "null"] }
                    },
                    "additionalProperties": true
                }
            },
            "additionalProperties": true
        },
        "ClaudeError": {
            "type": "object",
            "description": "Error envelope for the Anthropic Messages routes.",
            "properties": {
                "type": { "type": "string", "const": "error" },
                "error": {
                    "type": "object",
                    "properties": {
                        "type": { "type": "string" },
                        "message": { "type": "string" }
                    },
                    "additionalProperties": true
                }
            },
            "additionalProperties": true
        },
        "RealtimeError": {
            "type": "object",
            "description": "Error envelope for the realtime routes \
    (`{\"error\": {\"message\", \"type\", \"code\", \"param\"}}`).",
            "properties": {
                "error": {
                    "type": "object",
                    "properties": {
                        "message": { "type": "string" },
                        "type": { "type": "string" },
                        "code": { "type": "string" },
                        "param": { "type": ["string", "null"] }
                    },
                    "additionalProperties": true
                }
            },
            "additionalProperties": true
        },
        "Model": {
            "type": "object",
            "description": "One [OI] or Grok Shell catalog entry; fields beyond the documented ones may be present.",
            "properties": {
                "id": { "type": "string" },
                "object": { "type": "string" },
                "created": { "type": "integer" },
                "owned_by": { "type": "string" },
                "model": { "type": "string" },
                "name": { "type": "string" },
                "context_window": { "type": "integer" },
                "api_backend": { "type": "string" },
                "supported_in_api": { "type": "boolean" },
                "reasoning_efforts": { "type": "array", "items": { "type": "object", "additionalProperties": true } }
            },
            "required": ["id"],
            "additionalProperties": true
        },
        "ModelList": {
            "type": "object",
            "description": "[OI] (default) and Grok Shell (`grok-shell` User-Agent) catalog: a top-level `object: list` with `data`.",
            "properties": {
                "object": { "type": "string", "const": "list" },
                "data": { "type": "array", "items": r("Model") }
            },
            "required": ["object", "data"],
            "additionalProperties": true
        },
        "ClaudeModel": {
            "type": "object",
            "description": "One Anthropic client catalog entry.",
            "properties": {
                "id": { "type": "string" },
                "object": { "type": "string" },
                "type": { "type": "string" },
                "owned_by": { "type": "string" },
                "display_name": { "type": "string" },
                "created_at": { "type": "string", "description": "RFC 3339 timestamp." },
                "max_input_tokens": { "type": "integer" },
                "max_tokens": { "type": "integer" }
            },
            "required": ["id"],
            "additionalProperties": true
        },
        "ClaudeModelList": {
            "type": "object",
            "description": "Anthropic client catalog (`anthropic-version` header or `claude-cli` User-Agent). It has NO top-level `object`: the body is `data` plus `has_more`, `first_id` and `last_id`.",
            "properties": {
                "data": { "type": "array", "items": r("ClaudeModel") },
                "has_more": { "type": "boolean" },
                "first_id": { "type": "string" },
                "last_id": { "type": "string" }
            },
            "required": ["data", "has_more", "first_id", "last_id"],
            "additionalProperties": true
        },
        "CodexModelCatalog": {
            "type": "object",
            "description": "Codex client catalog returned when the `client_version` query key is present: a top-level `models` array.",
            "properties": {
                "models": { "type": "array", "items": { "type": "object", "additionalProperties": true } }
            },
            "required": ["models"],
            "additionalProperties": true
        },
        "GeminiModel": {
            "type": "object",
            "description": "One Gemini catalog entry.",
            "properties": {
                "name": { "type": "string" },
                "displayName": { "type": "string" },
                "description": { "type": "string" },
                "supportedGenerationMethods": { "type": "array", "items": { "type": "string" } }
            },
            "required": ["name"],
            "additionalProperties": true
        },
        "GeminiModelList": {
            "type": "object",
            "properties": {
                "models": { "type": "array", "items": r("GeminiModel") }
            },
            "required": ["models"],
            "additionalProperties": true
        },
        "AlphaSearchRequest": {
            "type": "object",
            "description": "Codex search payload in Codex search format; forwarded unchanged.",
            "properties": {
                "id": { "type": "string" },
                "model": { "type": "string" },
                "query": { "type": "string" }
            },
            "additionalProperties": true
        },
        "InteractionsRequest": {
            "type": "object",
            "description": "Gemini Interactions request. Exactly one of `model` or `agent` is required.",
            "properties": {
                "model": { "type": "string" },
                "agent": { "type": "string" },
                "input": {},
                "stream": { "type": "boolean" }
            },
            "additionalProperties": true
        },
        "ClientSecretRequest": {
            "type": "object",
            "description": "Realtime client-secret request: a session configuration and an optional lifetime.",
            "properties": {
                "session": { "type": "object", "additionalProperties": true },
                "expires_after": {
                    "type": "object",
                    "properties": {
                        "anchor": { "type": "string" },
                        "seconds": { "type": "integer" }
                    },
                    "additionalProperties": true
                }
            },
            "additionalProperties": true
        },
        "ClientSecretResponse": {
            "type": "object",
            "properties": {
                "value": { "type": "string", "description": "The ephemeral `ek_...` client secret." },
                "expires_at": { "type": "integer" },
                "session": { "type": "object", "additionalProperties": true }
            },
            "required": ["value", "expires_at", "session"],
            "additionalProperties": true
        }
    })
}

fn paths() -> Value {
    let items: Vec<(&'static str, Value)> = vec![
        (
            "/healthz",
            path_item(vec![(
                "get",
                op(
                    "healthz",
                    TAG_GENERAL,
                    "Liveness probe",
                    public_security(),
                    vec![],
                    None,
                    vec![(
                        "200",
                        resp(
                            "Service is up. HEAD is also registered and returns an empty body.",
                            &[(
                                JSON,
                                json!({ "type": "object", "properties": { "status": { "type": "string" } }, "required": ["status"] }),
                            )],
                        ),
                    )],
                ),
            )]),
        ),
        (
            "/",
            path_item(vec![(
                "get",
                op(
                    "root",
                    TAG_GENERAL,
                    "Server banner",
                    public_security(),
                    vec![],
                    None,
                    vec![(
                        "200",
                        resp(
                            "Endpoint list and server name.",
                            &[(
                                JSON,
                                json!({ "type": "object", "properties": { "endpoints": { "type": "array", "items": { "type": "string" } }, "message": { "type": "string" } }, "required": ["endpoints", "message"] }),
                            )],
                        ),
                    )],
                ),
            )]),
        ),
        (
            "/anthropic/callback",
            path_item(vec![(
                "get",
                op(
                    "anthropicCallback",
                    TAG_CALLBACK,
                    "Anthropic OAuth redirect",
                    public_security(),
                    vec![
                        query_param("code", "Authorization code"),
                        query_param("state", "OAuth state"),
                        query_param("error", "Error code from the provider"),
                    ],
                    None,
                    vec![(
                        "200",
                        resp("Authentication success page", &[(HTML, json!({ "type": "string" }))]),
                    )],
                ),
            )]),
        ),
        (
            "/codex/callback",
            path_item(vec![(
                "get",
                op(
                    "codexCallback",
                    TAG_CALLBACK,
                    "Codex OAuth redirect",
                    public_security(),
                    vec![
                        query_param("code", "Authorization code"),
                        query_param("state", "OAuth state"),
                        query_param("error", "Error code from the provider"),
                    ],
                    None,
                    vec![(
                        "200",
                        resp("Authentication success page", &[(HTML, json!({ "type": "string" }))]),
                    )],
                ),
            )]),
        ),
        (
            "/antigravity/callback",
            path_item(vec![(
                "get",
                op(
                    "antigravityCallback",
                    TAG_CALLBACK,
                    "Antigravity OAuth redirect",
                    public_security(),
                    vec![
                        query_param("code", "Authorization code"),
                        query_param("state", "OAuth state"),
                        query_param("error", "Error code from the provider"),
                    ],
                    None,
                    vec![(
                        "200",
                        resp("Authentication success page", &[(HTML, json!({ "type": "string" }))]),
                    )],
                ),
            )]),
        ),
        (
            "/callback",
            path_item(vec![(
                "get",
                op(
                    "devinCallback",
                    TAG_CALLBACK,
                    "Devin OAuth redirect",
                    public_security(),
                    vec![
                        query_param("code", "Authorization code"),
                        query_param("state", "OAuth state"),
                        query_param("error", "Error code from the provider"),
                    ],
                    None,
                    vec![
                        (
                            "200",
                            resp("Authentication success page", &[(HTML, json!({ "type": "string" }))]),
                        ),
                        (
                            "400",
                            resp(
                                "Missing `code` and `error`, or an invalid or expired callback",
                                &[(JSON, r("Error"))],
                            ),
                        ),
                    ],
                ),
            )]),
        ),
        (
            "/devin/callback",
            path_item(vec![(
                "get",
                op(
                    "devinCallbackAlias",
                    TAG_CALLBACK,
                    "Devin OAuth redirect (alias)",
                    public_security(),
                    vec![
                        query_param("code", "Authorization code"),
                        query_param("state", "OAuth state"),
                        query_param("error", "Error code from the provider"),
                    ],
                    None,
                    vec![
                        (
                            "200",
                            resp("Authentication success page", &[(HTML, json!({ "type": "string" }))]),
                        ),
                        (
                            "400",
                            resp(
                                "Missing `code` and `error`, or an invalid or expired callback",
                                &[(JSON, r("Error"))],
                            ),
                        ),
                    ],
                ),
            )]),
        ),
        (
            "/v1/models",
            path_item(vec![(
                "get",
                op(
                    "listModels",
                    TAG_OPENAI,
                    "List models",
                    client_security(),
                    vec![
                        query_param(
                            "client_version",
                            "Codex client version; selects the Codex client catalog",
                        ),
                        query_param("key", "Client key (alternative to the Authorization header)"),
                        query_param("auth_token", "Client key (alternative to the Authorization header)"),
                    ],
                    None,
                    with_errors(
                        vec![(
                            "200",
                            resp(
                                "Model catalog. The shape is selected by the request headers and is one of several variants (see the schema `oneOf`): Anthropic (`anthropic-version` header or `claude-cli` User-Agent), Grok Shell (`grok-shell` User-Agent) or the default OpenAI list.",
                                &[(JSON, model_list_schema())],
                            ),
                        )],
                        "Error",
                    ),
                ),
            )]),
        ),
        (
            "/v1/chat/completions",
            path_item(vec![(
                "post",
                op(
                    "chatCompletions",
                    TAG_OPENAI,
                    "Chat completions",
                    client_security(),
                    vec![],
                    Some(body_content(&[(JSON, r("JsonBody"))])),
                    with_errors(
                        vec![(
                            "200",
                            resp(
                                "Chat completion. Streams `text/event-stream` when `stream` is true.",
                                &[(JSON, r("JsonBody")), (SSE, r("JsonBody"))],
                            ),
                        )],
                        "Error",
                    ),
                ),
            )]),
        ),
        (
            "/v1/completions",
            path_item(vec![(
                "post",
                op(
                    "completions",
                    TAG_OPENAI,
                    "Legacy completions",
                    client_security(),
                    vec![],
                    Some(body_content(&[(JSON, r("JsonBody"))])),
                    with_errors(
                        vec![(
                            "200",
                            resp(
                                "Text completion. Streams `text/event-stream` when `stream` is true.",
                                &[(JSON, r("JsonBody")), (SSE, r("JsonBody"))],
                            ),
                        )],
                        "Error",
                    ),
                ),
            )]),
        ),
        (
            "/v1/responses",
            path_item(vec![
                (
                    "post",
                    op(
                        "createResponse",
                        TAG_OPENAI,
                        "Create a response",
                        client_security(),
                        vec![],
                        Some(body_content(&[(JSON, r("JsonBody"))])),
                        with_errors(
                            vec![(
                                "200",
                                resp(
                                    "Response. Streams `text/event-stream` when `stream` is true.",
                                    &[(JSON, r("JsonBody")), (SSE, r("JsonBody"))],
                                ),
                            )],
                            "Error",
                        ),
                    ),
                ),
                (
                    "get",
                    op(
                        "responsesWebSocket",
                        TAG_WEBSOCKET,
                        "Responses WebSocket",
                        client_security(),
                        vec![],
                        None,
                        vec![
                            (
                                "101",
                                resp(
                                    "Switching Protocols: the connection is upgraded to a WebSocket carrying Responses frames.",
                                    &[],
                                ),
                            ),
                            (
                                "400",
                                resp("Not a WebSocket handshake", &[(TEXT, json!({ "type": "string" }))]),
                            ),
                        ],
                    ),
                ),
            ]),
        ),
        (
            "/v1/responses/compact",
            path_item(vec![(
                "post",
                op(
                    "compactResponse",
                    TAG_OPENAI,
                    "Compact a response",
                    client_security(),
                    vec![],
                    Some(body_content(&[(JSON, r("JsonBody"))])),
                    with_errors(
                        vec![("200", resp("Compacted response", &[(JSON, r("JsonBody"))]))],
                        "Error",
                    ),
                ),
            )]),
        ),
        (
            "/backend-api/codex/responses",
            path_item(vec![
                (
                    "post",
                    op(
                        "codexBackendResponses",
                        TAG_OPENAI,
                        "Create a Codex backend response",
                        client_security(),
                        vec![],
                        Some(body_content(&[(JSON, r("JsonBody"))])),
                        with_errors(
                            vec![(
                                "200",
                                resp(
                                    "Response. Streams `text/event-stream` when `stream` is true.",
                                    &[(JSON, r("JsonBody")), (SSE, r("JsonBody"))],
                                ),
                            )],
                            "Error",
                        ),
                    ),
                ),
                (
                    "get",
                    op(
                        "codexBackendResponsesWebSocket",
                        TAG_WEBSOCKET,
                        "Codex backend Responses WebSocket",
                        client_security(),
                        vec![],
                        None,
                        vec![
                            (
                                "101",
                                resp(
                                    "Switching Protocols: the connection is upgraded to a WebSocket carrying Responses frames.",
                                    &[],
                                ),
                            ),
                            (
                                "400",
                                resp("Not a WebSocket handshake", &[(TEXT, json!({ "type": "string" }))]),
                            ),
                        ],
                    ),
                ),
            ]),
        ),
        (
            "/backend-api/codex/responses/compact",
            path_item(vec![(
                "post",
                op(
                    "codexBackendResponsesCompact",
                    TAG_OPENAI,
                    "Compact a Codex backend response",
                    client_security(),
                    vec![],
                    Some(body_content(&[(JSON, r("JsonBody"))])),
                    with_errors(
                        vec![("200", resp("Compacted response", &[(JSON, r("JsonBody"))]))],
                        "Error",
                    ),
                ),
            )]),
        ),
        (
            "/v1/messages",
            path_item(vec![(
                "post",
                op(
                    "claudeMessages",
                    TAG_CLAUDE,
                    "Anthropic Messages",
                    client_security(),
                    vec![query_param("alt", "Response format hint (`sse` or empty means SSE)")],
                    Some(body_content(&[(JSON, r("JsonBody"))])),
                    with_errors(
                        vec![(
                            "200",
                            resp(
                                "Message. Streams `text/event-stream` unless `stream` is false.",
                                &[(JSON, r("JsonBody")), (SSE, r("JsonBody"))],
                            ),
                        )],
                        "ClaudeError",
                    ),
                ),
            )]),
        ),
        (
            "/v1/messages/count_tokens",
            path_item(vec![(
                "post",
                op(
                    "claudeCountTokens",
                    TAG_CLAUDE,
                    "Count tokens",
                    client_security(),
                    vec![],
                    Some(body_content(&[(JSON, r("JsonBody"))])),
                    with_errors(
                        vec![("200", resp("Token count", &[(JSON, r("JsonBody"))]))],
                        "ClaudeError",
                    ),
                ),
            )]),
        ),
        (
            "/v1beta/models",
            path_item(vec![(
                "get",
                op(
                    "geminiListModels",
                    TAG_GEMINI,
                    "List Gemini models",
                    client_security(),
                    vec![],
                    None,
                    with_errors(
                        vec![("200", resp("Gemini model catalog", &[(JSON, r("GeminiModelList"))]))],
                        "Error",
                    ),
                ),
            )]),
        ),
        (
            "/v1beta/interactions",
            path_item(vec![(
                "post",
                op(
                    "geminiInteractions",
                    TAG_GEMINI,
                    "Gemini Interactions",
                    client_security(),
                    vec![],
                    Some(body_content(&[(JSON, r("InteractionsRequest"))])),
                    with_errors(
                        vec![(
                            "200",
                            resp(
                                "Interaction. Streams `text/event-stream` when `stream` is true.",
                                &[(JSON, r("JsonBody")), (SSE, r("JsonBody"))],
                            ),
                        )],
                        "Error",
                    ),
                ),
            )]),
        ),
        (
            "/v1beta/models/{action}",
            path_item(vec![
                (
                    "post",
                    op(
                        "geminiGenerateContent",
                        TAG_GEMINI,
                        "Gemini generateContent / streamGenerateContent / countTokens",
                        client_security(),
                        vec![
                            path_param(
                                "action",
                                "The model and colon action, e.g. `gemini-2.5-flash:generateContent`, `gemini-2.5-flash:streamGenerateContent`, `gemini-2.5-flash:countTokens`. A single trailing segment with no colon answers an empty 200.",
                            ),
                            query_param("alt", "When set, streaming frames are emitted raw instead of as SSE"),
                        ],
                        Some(body_content(&[(JSON, r("JsonBody"))])),
                        with_errors(
                            vec![(
                                "200",
                                resp(
                                    "Generated content. `streamGenerateContent` answers `text/event-stream`; a raw `alt` answers the upstream content type.",
                                    &[(JSON, r("JsonBody")), (SSE, r("JsonBody"))],
                                ),
                            )],
                            "Error",
                        ),
                    ),
                ),
                (
                    "get",
                    op(
                        "geminiGetModel",
                        TAG_GEMINI,
                        "Get a Gemini model",
                        client_security(),
                        vec![path_param(
                            "action",
                            "The model name, optionally prefixed with `models/`",
                        )],
                        None,
                        with_errors(
                            vec![("200", resp("Model description", &[(JSON, r("GeminiModel"))]))],
                            "Error",
                        ),
                    ),
                ),
            ]),
        ),
        (
            "/v1/images/generations",
            path_item(vec![(
                "post",
                op(
                    "imageGenerations",
                    TAG_IMAGES,
                    "Generate images",
                    client_security(),
                    vec![],
                    Some(body_content(&[(JSON, r("JsonBody"))])),
                    with_errors(
                        vec![(
                            "200",
                            resp(
                                "Generated images. Streams `text/event-stream` when `stream` is true.",
                                &[(JSON, r("JsonBody")), (SSE, r("JsonBody"))],
                            ),
                        )],
                        "Error",
                    ),
                ),
            )]),
        ),
        (
            "/v1/images/edits",
            path_item(vec![(
                "post",
                op(
                    "imageEdits",
                    TAG_IMAGES,
                    "Edit images",
                    client_security(),
                    vec![],
                    Some(body_content(&[(JSON, r("JsonBody")), (MULTIPART, r("JsonBody"))])),
                    with_errors(vec![("200", resp("Edited images", &[(JSON, r("JsonBody"))]))], "Error"),
                ),
            )]),
        ),
        (
            "/v1/videos",
            path_item(vec![(
                "post",
                op(
                    "videosCreate",
                    TAG_VIDEOS,
                    "Create a video",
                    client_security(),
                    vec![],
                    Some(body_content(&[(JSON, r("JsonBody"))])),
                    with_errors(vec![("200", resp("Video job", &[(JSON, r("JsonBody"))]))], "Error"),
                ),
            )]),
        ),
        (
            "/v1/videos/generations",
            path_item(vec![(
                "post",
                op(
                    "videosGenerations",
                    TAG_VIDEOS,
                    "Generate a video",
                    client_security(),
                    vec![],
                    Some(body_content(&[(JSON, r("JsonBody"))])),
                    with_errors(vec![("200", resp("Video job", &[(JSON, r("JsonBody"))]))], "Error"),
                ),
            )]),
        ),
        (
            "/v1/videos/edits",
            path_item(vec![(
                "post",
                op(
                    "videosEdits",
                    TAG_VIDEOS,
                    "Edit a video",
                    client_security(),
                    vec![],
                    Some(body_content(&[(JSON, r("JsonBody"))])),
                    with_errors(vec![("200", resp("Video job", &[(JSON, r("JsonBody"))]))], "Error"),
                ),
            )]),
        ),
        (
            "/v1/videos/extensions",
            path_item(vec![(
                "post",
                op(
                    "videosExtensions",
                    TAG_VIDEOS,
                    "Extend a video",
                    client_security(),
                    vec![],
                    Some(body_content(&[(JSON, r("JsonBody"))])),
                    with_errors(vec![("200", resp("Video job", &[(JSON, r("JsonBody"))]))], "Error"),
                ),
            )]),
        ),
        (
            "/v1/videos/{request_id}",
            path_item(vec![(
                "get",
                op(
                    "videosRetrieve",
                    TAG_VIDEOS,
                    "Retrieve a video job",
                    client_security(),
                    vec![path_param("request_id", "The xAI video request id")],
                    None,
                    with_errors(
                        vec![("200", resp("Video job status", &[(JSON, r("JsonBody"))]))],
                        "Error",
                    ),
                ),
            )]),
        ),
        (
            "/openai/v1/videos",
            path_item(vec![(
                "post",
                op(
                    "openaiVideosCreate",
                    TAG_VIDEOS,
                    "Create a video (OpenAI-compatible)",
                    client_security(),
                    vec![],
                    Some(body_content(&[
                        (JSON, r("JsonBody")),
                        (MULTIPART, r("JsonBody")),
                        (FORM, r("JsonBody")),
                    ])),
                    with_errors(vec![("200", resp("Video job", &[(JSON, r("JsonBody"))]))], "Error"),
                ),
            )]),
        ),
        (
            "/openai/v1/videos/{video_id}",
            path_item(vec![(
                "get",
                op(
                    "openaiVideosRetrieve",
                    TAG_VIDEOS,
                    "Retrieve a video (OpenAI-compatible)",
                    client_security(),
                    vec![path_param("video_id", "The video id")],
                    None,
                    with_errors(vec![("200", resp("Video status", &[(JSON, r("JsonBody"))]))], "Error"),
                ),
            )]),
        ),
        (
            "/openai/v1/videos/{video_id}/content",
            path_item(vec![(
                "get",
                op(
                    "openaiVideosContent",
                    TAG_VIDEOS,
                    "Download video content (OpenAI-compatible)",
                    client_security(),
                    vec![
                        path_param("video_id", "The video id"),
                        query_param("variant", "Download variant; only `video` is supported"),
                    ],
                    None,
                    with_errors(
                        vec![(
                            "200",
                            resp(
                                "Video file bytes",
                                &[(OCTET, json!({ "type": "string", "format": "binary" }))],
                            ),
                        )],
                        "Error",
                    ),
                ),
            )]),
        ),
        (
            "/v1/alpha/search",
            path_item(vec![(
                "post",
                op(
                    "codexAlphaSearch",
                    TAG_CODEX,
                    "Codex Alpha Search",
                    client_security(),
                    vec![],
                    Some(body_content(&[(JSON, r("AlphaSearchRequest"))])),
                    with_errors(
                        vec![(
                            "200",
                            resp(
                                "Search result (upstream status and body are passed through)",
                                &[(JSON, r("JsonBody"))],
                            ),
                        )],
                        "Error",
                    ),
                ),
            )]),
        ),
        (
            "/backend-api/codex/alpha/search",
            path_item(vec![(
                "post",
                op(
                    "codexBackendAlphaSearch",
                    TAG_CODEX,
                    "Codex Alpha Search (backend path)",
                    client_security(),
                    vec![],
                    Some(body_content(&[(JSON, r("AlphaSearchRequest"))])),
                    with_errors(
                        vec![(
                            "200",
                            resp(
                                "Search result (upstream status and body are passed through)",
                                &[(JSON, r("JsonBody"))],
                            ),
                        )],
                        "Error",
                    ),
                ),
            )]),
        ),
        (
            "/v1/live",
            path_item(vec![(
                "post",
                op(
                    "liveCall",
                    TAG_REALTIME,
                    "Live WebRTC call negotiation",
                    client_security(),
                    vec![],
                    Some(live_call_request_content()),
                    with_errors(
                        vec![(
                            "201",
                            resp(
                                "WebRTC answer",
                                &[(SDP, json!({ "type": "string" })), (JSON, r("JsonBody"))],
                            ),
                        )],
                        "Error",
                    ),
                ),
            )]),
        ),
        (
            "/v1/live/{call_id}",
            path_item(vec![(
                "get",
                op(
                    "liveSideband",
                    TAG_WEBSOCKET,
                    "Live sideband WebSocket",
                    client_security(),
                    vec![path_param("call_id", "The live call id")],
                    None,
                    vec![
                        (
                            "101",
                            resp(
                                "Switching Protocols: the connection is upgraded to the call's sideband WebSocket.",
                                &[],
                            ),
                        ),
                        (
                            "400",
                            resp("Not a WebSocket handshake", &[(TEXT, json!({ "type": "string" }))]),
                        ),
                    ],
                ),
            )]),
        ),
        (
            "/v1/realtime",
            path_item(vec![
                (
                    "post",
                    op(
                        "realtimeCall",
                        TAG_REALTIME,
                        "Realtime WebRTC call negotiation",
                        realtime_security(),
                        vec![],
                        Some(live_call_request_content()),
                        with_errors(
                            vec![(
                                "201",
                                resp(
                                    "WebRTC answer",
                                    &[(SDP, json!({ "type": "string" })), (JSON, r("JsonBody"))],
                                ),
                            )],
                            "RealtimeError",
                        ),
                    ),
                ),
                (
                    "get",
                    op(
                        "realtimeSocket",
                        TAG_WEBSOCKET,
                        "Realtime WebSocket",
                        realtime_security(),
                        vec![query_param(
                            "call_id",
                            "Attach to a remembered call's sideband instead of a new standard Realtime session",
                        )],
                        None,
                        vec![
                            (
                                "101",
                                resp(
                                    "Switching Protocols: the connection is upgraded to a Realtime WebSocket.",
                                    &[],
                                ),
                            ),
                            (
                                "400",
                                resp("Not a WebSocket handshake", &[(TEXT, json!({ "type": "string" }))]),
                            ),
                        ],
                    ),
                ),
            ]),
        ),
        (
            "/v1/realtime/calls",
            path_item(vec![(
                "post",
                op(
                    "realtimeCalls",
                    TAG_REALTIME,
                    "Realtime WebRTC call negotiation",
                    realtime_security(),
                    vec![],
                    Some(live_call_request_content()),
                    with_errors(
                        vec![(
                            "201",
                            resp(
                                "WebRTC answer",
                                &[(SDP, json!({ "type": "string" })), (JSON, r("JsonBody"))],
                            ),
                        )],
                        "RealtimeError",
                    ),
                ),
            )]),
        ),
        (
            "/v1/realtime/calls/{call_id}",
            path_item(vec![(
                "get",
                op(
                    "realtimeCallsSideband",
                    TAG_WEBSOCKET,
                    "Realtime call sideband WebSocket",
                    realtime_security(),
                    vec![path_param("call_id", "The realtime call id")],
                    None,
                    vec![
                        (
                            "101",
                            resp(
                                "Switching Protocols: the connection is upgraded to the call's sideband WebSocket.",
                                &[],
                            ),
                        ),
                        (
                            "400",
                            resp("Not a WebSocket handshake", &[(TEXT, json!({ "type": "string" }))]),
                        ),
                    ],
                ),
            )]),
        ),
        (
            "/v1/realtime/client_secrets",
            path_item(vec![(
                "post",
                op(
                    "realtimeClientSecrets",
                    TAG_REALTIME,
                    "Issue an ephemeral client secret",
                    client_security(),
                    vec![],
                    Some(body_content(&[(JSON, r("ClientSecretRequest"))])),
                    with_errors(
                        vec![(
                            "200",
                            resp("Ephemeral client secret", &[(JSON, r("ClientSecretResponse"))]),
                        )],
                        "RealtimeError",
                    ),
                ),
            )]),
        ),
        (
            "/v1/realtime/sessions",
            path_item(vec![(
                "post",
                op(
                    "realtimeSessions",
                    TAG_REALTIME,
                    "Issue an ephemeral client secret (legacy session)",
                    client_security(),
                    vec![],
                    Some(body_content(&[(JSON, r("JsonBody"))])),
                    with_errors(
                        vec![(
                            "200",
                            resp("Session with an embedded client secret", &[(JSON, r("JsonBody"))]),
                        )],
                        "RealtimeError",
                    ),
                ),
            )]),
        ),
        (
            "/v1/realtime/transcription_sessions",
            path_item(vec![(
                "post",
                op(
                    "realtimeTranscriptionSessions",
                    TAG_REALTIME,
                    "Transcription-only sessions (not supported)",
                    client_security(),
                    vec![],
                    Some(body_content(&[(JSON, r("JsonBody"))])),
                    vec![(
                        "501",
                        resp(
                            "Transcription-only sessions are not supported by the ChatGPT/Codex OAuth upstream",
                            &[(JSON, r("RealtimeError"))],
                        ),
                    )],
                ),
            )]),
        ),
        (
            "/v1/realtime/translations",
            path_item(vec![
                (
                    "get",
                    op(
                        "realtimeTranslationsGet",
                        TAG_REALTIME,
                        "Realtime translation sessions (not supported)",
                        realtime_security(),
                        vec![],
                        None,
                        vec![(
                            "501",
                            resp(
                                "Realtime translation sessions are not supported by the ChatGPT/Codex OAuth upstream",
                                &[(JSON, r("RealtimeError"))],
                            ),
                        )],
                    ),
                ),
                (
                    "post",
                    op(
                        "realtimeTranslationsPost",
                        TAG_REALTIME,
                        "Realtime translation sessions (not supported)",
                        realtime_security(),
                        vec![],
                        Some(body_content(&[(JSON, r("JsonBody"))])),
                        vec![(
                            "501",
                            resp(
                                "Realtime translation sessions are not supported by the ChatGPT/Codex OAuth upstream",
                                &[(JSON, r("RealtimeError"))],
                            ),
                        )],
                    ),
                ),
            ]),
        ),
        (
            "/v1/realtime/translations/client_secrets",
            path_item(vec![(
                "post",
                op(
                    "realtimeTranslationClientSecrets",
                    TAG_REALTIME,
                    "Translation client secrets (not supported)",
                    client_security(),
                    vec![],
                    Some(body_content(&[(JSON, r("JsonBody"))])),
                    vec![(
                        "501",
                        resp(
                            "Realtime translation sessions are not supported by the ChatGPT/Codex OAuth upstream",
                            &[(JSON, r("RealtimeError"))],
                        ),
                    )],
                ),
            )]),
        ),
        (
            "/v1/realtime/calls/{call_id}/hangup",
            path_item(vec![(
                "post",
                op(
                    "realtimeHangup",
                    TAG_REALTIME,
                    "Hang up a realtime call",
                    client_security(),
                    vec![path_param("call_id", "The realtime call id")],
                    None,
                    with_errors(
                        vec![("200", resp("Hangup result", &[(JSON, r("JsonBody"))]))],
                        "RealtimeError",
                    ),
                ),
            )]),
        ),
        (
            "/v1/realtime/calls/{call_id}/accept",
            path_item(vec![(
                "post",
                op(
                    "realtimeCallAccept",
                    TAG_REALTIME,
                    "Accept a SIP call (not supported)",
                    client_security(),
                    vec![path_param("call_id", "The realtime call id")],
                    None,
                    vec![(
                        "501",
                        resp(
                            "Realtime SIP accept is not supported by the ChatGPT/Codex OAuth upstream",
                            &[(JSON, r("RealtimeError"))],
                        ),
                    )],
                ),
            )]),
        ),
        (
            "/v1/realtime/calls/{call_id}/reject",
            path_item(vec![(
                "post",
                op(
                    "realtimeCallReject",
                    TAG_REALTIME,
                    "Reject a SIP call (not supported)",
                    client_security(),
                    vec![path_param("call_id", "The realtime call id")],
                    None,
                    vec![(
                        "501",
                        resp(
                            "Realtime SIP reject is not supported by the ChatGPT/Codex OAuth upstream",
                            &[(JSON, r("RealtimeError"))],
                        ),
                    )],
                ),
            )]),
        ),
        (
            "/v1/realtime/calls/{call_id}/refer",
            path_item(vec![(
                "post",
                op(
                    "realtimeCallRefer",
                    TAG_REALTIME,
                    "Refer a SIP call (not supported)",
                    client_security(),
                    vec![path_param("call_id", "The realtime call id")],
                    None,
                    vec![(
                        "501",
                        resp(
                            "Realtime SIP refer is not supported by the ChatGPT/Codex OAuth upstream",
                            &[(JSON, r("RealtimeError"))],
                        ),
                    )],
                ),
            )]),
        ),
        (
            "/v1/ws",
            path_item(vec![(
                "get",
                op(
                    "aistudioRelayWebSocket",
                    TAG_WEBSOCKET,
                    "AI Studio relay WebSocket",
                    optional_client_security(),
                    vec![],
                    None,
                    vec![
                        (
                            "101",
                            resp(
                                "Switching Protocols: the connection is upgraded to the AI Studio relay WebSocket. Client keys are required only while `ws-auth` is on (the default).",
                                &[],
                            ),
                        ),
                        (
                            "400",
                            resp("Not a WebSocket handshake", &[(TEXT, json!({ "type": "string" }))]),
                        ),
                    ],
                ),
            )]),
        ),
    ];
    let mut m = Map::new();
    for (path, item) in items {
        m.insert(path.into(), item);
    }
    Value::Object(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn doc() -> Value {
        document()
    }

    fn collect_refs(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                if let Some(Value::String(reference)) = map.get("$ref") {
                    out.push(reference.clone());
                }
                for child in map.values() {
                    collect_refs(child, out);
                }
            }
            Value::Array(items) => {
                for child in items {
                    collect_refs(child, out);
                }
            }
            _ => {}
        }
    }

    fn resolves(root: &Value, pointer: &str) -> bool {
        let Some(rest) = pointer.strip_prefix("#/") else {
            return false;
        };
        let mut cur = root;
        for segment in rest.split('/') {
            let segment = segment.replace("~1", "/").replace("~0", "~");
            match cur {
                Value::Object(map) => match map.get(&segment) {
                    Some(next) => cur = next,
                    None => return false,
                },
                Value::Array(items) => match segment.parse::<usize>().ok().and_then(|i| items.get(i)) {
                    Some(next) => cur = next,
                    None => return false,
                },
                _ => return false,
            }
        }
        true
    }

    fn dangling_refs(root: &Value) -> Vec<String> {
        let mut refs = Vec::new();
        collect_refs(root, &mut refs);
        refs.into_iter().filter(|r| !resolves(root, r)).collect()
    }

    const METHODS: [&str; 8] = ["get", "put", "post", "delete", "options", "head", "patch", "trace"];

    fn operations(root: &Value) -> Vec<(String, String, Value)> {
        let mut out = Vec::new();
        if let Some(paths) = root["paths"].as_object() {
            for (path, item) in paths {
                if let Some(item) = item.as_object() {
                    for (method, operation) in item {
                        if METHODS.contains(&method.as_str()) {
                            out.push((path.clone(), method.clone(), operation.clone()));
                        }
                    }
                }
            }
        }
        out
    }

    fn operation_ids(root: &Value) -> Vec<String> {
        operations(root)
            .into_iter()
            .filter_map(|(_, _, operation)| operation["operationId"].as_str().map(str::to_owned))
            .collect()
    }

    fn duplicate_operation_ids(root: &Value) -> Vec<String> {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for id in operation_ids(root) {
            *counts.entry(id).or_insert(0) += 1;
        }
        counts.into_iter().filter(|(_, n)| *n > 1).map(|(id, _)| id).collect()
    }

    fn operations_without_id(root: &Value) -> Vec<String> {
        operations(root)
            .into_iter()
            .filter(|(_, _, operation)| operation["operationId"].as_str().is_none_or(str::is_empty))
            .map(|(path, method, _)| format!("{method} {path}"))
            .collect()
    }

    fn placeholders(path: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut rest = path;
        while let Some(start) = rest.find('{') {
            let Some(end) = rest[start..].find('}') else {
                break;
            };
            out.push(rest[start + 1..start + end].to_owned());
            rest = &rest[start + end + 1..];
        }
        out
    }

    fn missing_path_params(root: &Value) -> Vec<String> {
        let mut out = Vec::new();
        let Some(paths) = root["paths"].as_object() else {
            return vec!["document has no `paths`".to_owned()];
        };
        for (path, item) in paths {
            let Some(item) = item.as_object() else {
                out.push(format!("{path}: path item is not an object"));
                continue;
            };
            for (method, operation) in item {
                if !METHODS.contains(&method.as_str()) {
                    continue;
                }
                let declared: Vec<&str> = operation["parameters"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|p| p["in"] == "path" && p["required"] == true)
                    .filter_map(|p| p["name"].as_str())
                    .collect();
                for name in placeholders(path) {
                    if !declared.contains(&name.as_str()) {
                        out.push(format!(
                            "{method} {path}: path parameter `{name}` is missing or not required"
                        ));
                    }
                }
            }
        }
        out
    }

    fn malformed_schemas(root: &Value) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(schemas) = root["components"]["schemas"].as_object() {
            for (name, schema) in schemas {
                let well_formed = schema.is_object() && (schema.get("type").is_some() || schema.get("$ref").is_some());
                if !well_formed {
                    out.push(name.clone());
                }
            }
        }
        out
    }

    fn scheme_names(security: &Value) -> Vec<String> {
        security
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|alternative| alternative.as_object().and_then(|m| m.keys().next().cloned()))
            .collect()
    }

    #[test]
    fn document_is_openapi_31_with_paths() {
        let doc = doc();
        assert_eq!(doc["openapi"], OPENAPI_VERSION);
        assert_eq!(doc["openapi"], "3.1.0");
        assert_eq!(doc["info"]["version"], env!("CARGO_PKG_VERSION"));
        assert!(doc["info"]["title"].as_str().is_some_and(|t| !t.is_empty()));
        assert!(
            doc["paths"].as_object().is_some_and(|p| !p.is_empty()),
            "paths must be nonempty"
        );
        assert!(
            doc["components"]["securitySchemes"]
                .as_object()
                .is_some_and(|s| !s.is_empty())
        );
    }

    #[test]
    fn operation_ids_are_unique_and_present() {
        let doc = doc();
        assert!(
            operations_without_id(&doc).is_empty(),
            "operations missing an operationId: {:?}",
            operations_without_id(&doc)
        );
        assert!(
            duplicate_operation_ids(&doc).is_empty(),
            "duplicate operationIds: {:?}",
            duplicate_operation_ids(&doc)
        );
        for id in operation_ids(&doc) {
            assert!(
                id.chars().next().is_some_and(|c| c.is_ascii_alphabetic()),
                "operationId must start with a letter: {id}"
            );
        }
    }

    #[test]
    fn local_refs_resolve() {
        let doc = doc();
        let dangling = dangling_refs(&doc);
        assert!(dangling.is_empty(), "dangling $refs: {dangling:?}");
    }

    #[test]
    fn every_path_placeholder_is_a_required_parameter() {
        let doc = doc();
        let missing = missing_path_params(&doc);
        assert!(missing.is_empty(), "path parameters not declared required: {missing:?}");
    }

    #[test]
    fn component_schemas_are_well_formed() {
        let doc = doc();
        let malformed = malformed_schemas(&doc);
        assert!(malformed.is_empty(), "malformed schemas: {malformed:?}");
    }

    #[test]
    fn validator_detects_malformed_documents() {
        let mut duplicate = doc();
        duplicate["paths"]["/v1/models"]["get"]["operationId"] = json!("chatCompletions");
        assert!(
            !duplicate_operation_ids(&duplicate).is_empty(),
            "duplicate operationId not detected"
        );

        let mut unnamed = doc();
        unnamed["paths"]["/healthz"]["get"]
            .as_object_mut()
            .unwrap()
            .remove("operationId");
        assert!(
            !operations_without_id(&unnamed).is_empty(),
            "missing operationId not detected"
        );

        let mut dangling = doc();
        dangling["components"]["schemas"]["Broken"] =
            json!({ "type": "object", "properties": { "x": { "$ref": "#/components/schemas/DoesNotExist" } } });
        assert!(!dangling_refs(&dangling).is_empty(), "dangling $ref not detected");

        let mut missing_param = doc();
        missing_param["paths"]["/v1/videos/{request_id}"]["get"]["parameters"] = json!([]);
        assert!(
            !missing_path_params(&missing_param).is_empty(),
            "missing required path parameter not detected"
        );

        let mut bad_schema = doc();
        bad_schema["components"]["schemas"]["Broken"] = json!({ "description": "no type" });
        assert!(
            !malformed_schemas(&bad_schema).is_empty(),
            "malformed schema not detected"
        );
    }

    #[test]
    fn representative_media_types_are_declared() {
        let doc = doc();
        let ok_content = |path: &str, method: &str| doc["paths"][path][method]["responses"]["200"]["content"].clone();
        let req_content = |path: &str, method: &str| doc["paths"][path][method]["requestBody"]["content"].clone();

        assert!(
            ok_content("/v1/chat/completions", "post")
                .get("text/event-stream")
                .is_some(),
            "chat must declare SSE"
        );
        assert!(
            ok_content("/v1/chat/completions", "post")
                .get("application/json")
                .is_some(),
            "chat must declare JSON"
        );
        assert!(
            ok_content("/v1/messages", "post").get("text/event-stream").is_some(),
            "messages must declare SSE"
        );
        assert!(
            ok_content("/v1beta/models/{action}", "post")
                .get("text/event-stream")
                .is_some(),
            "gemini stream must declare SSE"
        );
        assert!(
            ok_content("/openai/v1/videos/{video_id}/content", "get")
                .get("application/octet-stream")
                .is_some(),
            "video content must be binary"
        );
        assert!(
            req_content("/v1/images/edits", "post")
                .get("multipart/form-data")
                .is_some(),
            "image edits must accept multipart"
        );
        assert!(
            req_content("/v1/live", "post").get("application/sdp").is_some(),
            "live call must accept SDP"
        );
        assert!(
            req_content("/openai/v1/videos", "post")
                .get("multipart/form-data")
                .is_some(),
            "openai videos must accept multipart"
        );
        assert!(
            ok_content("/v1/realtime/client_secrets", "post")
                .get("application/json")
                .is_some(),
            "client secrets must be JSON"
        );
        assert!(
            doc["paths"]["/v1/responses"]["get"]["responses"]["101"].is_object(),
            "websocket upgrade must document 101"
        );
    }

    #[test]
    fn websocket_upgrade_endpoints_are_documented() {
        let doc = doc();
        for path in [
            "/v1/responses",
            "/backend-api/codex/responses",
            "/v1/ws",
            "/v1/live/{call_id}",
            "/v1/realtime",
            "/v1/realtime/calls/{call_id}",
        ] {
            assert!(
                doc["paths"][path]["get"]["responses"]["101"].is_object(),
                "{path} must document a 101 upgrade"
            );
        }
    }

    #[test]
    fn security_schemes_cover_every_client_key_candidate() {
        let schemes = doc()["components"]["securitySchemes"].clone();
        assert_eq!(schemes["clientBearer"]["type"], "http");
        assert_eq!(schemes["clientBearer"]["scheme"], "bearer");
        assert_eq!(schemes["clientGoogApiKey"]["type"], "apiKey");
        assert_eq!(schemes["clientGoogApiKey"]["name"], "X-Goog-Api-Key");
        assert_eq!(schemes["clientApiKey"]["name"], "X-Api-Key");
        assert_eq!(schemes["clientQueryKey"]["in"], "query");
        assert_eq!(schemes["clientQueryKey"]["name"], "key");
        assert_eq!(schemes["clientQueryAuthToken"]["name"], "auth_token");
        assert_eq!(schemes["realtimeClientSecret"]["scheme"], "bearer");
    }

    #[test]
    fn protected_paths_offer_the_client_key_alternatives() {
        let doc = doc();
        let names = scheme_names(&doc["paths"]["/v1/models"]["get"]["security"]);
        for expected in [
            "clientBearer",
            "clientGoogApiKey",
            "clientApiKey",
            "clientQueryKey",
            "clientQueryAuthToken",
        ] {
            assert!(
                names.contains(&expected.to_owned()),
                "OR must offer {expected}, got {names:?}"
            );
        }
        assert_eq!(
            doc["paths"]["/healthz"]["get"]["security"],
            json!([]),
            "public path must be open"
        );
        assert_eq!(doc["paths"]["/"]["get"]["security"], json!([]));
        assert_eq!(doc["paths"]["/anthropic/callback"]["get"]["security"], json!([]));
    }

    #[test]
    fn realtime_call_routes_accept_the_ephemeral_secret() {
        let doc = doc();
        let ephemeral = |path: &str, method: &str| {
            scheme_names(&doc["paths"][path][method]["security"]).contains(&"realtimeClientSecret".to_owned())
        };
        assert!(
            ephemeral("/v1/realtime", "post"),
            "realtime call must accept the ephemeral secret"
        );
        assert!(ephemeral("/v1/realtime/calls", "post"));
        assert!(ephemeral("/v1/realtime/calls/{call_id}", "get"));
        assert!(ephemeral("/v1/realtime", "get"));
        // The standard (secret-issuing) and ordinary routes take configured keys only.
        assert!(!ephemeral("/v1/realtime/client_secrets", "post"));
        assert!(!ephemeral("/v1/live", "post"));
    }

    fn required_keys(schema: &Value) -> Vec<String> {
        schema["required"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect()
    }

    fn has_keys(value: &Value, keys: &[String]) -> bool {
        keys.iter().all(|k| value.get(k).is_some())
    }

    #[test]
    fn live_call_routes_accept_multipart_and_raw_sdp() {
        let doc = doc();
        for (path, method) in [
            ("/v1/live", "post"),
            ("/v1/realtime", "post"),
            ("/v1/realtime/calls", "post"),
        ] {
            let content = &doc["paths"][path][method]["requestBody"]["content"];
            for media in [
                "multipart/form-data",
                "application/sdp",
                "text/plain",
                "application/json",
            ] {
                assert!(content.get(media).is_some(), "{path} must accept {media}");
            }
            let multipart = &content["multipart/form-data"]["schema"];
            assert!(
                required_keys(multipart).contains(&"sdp".to_owned()),
                "{path} multipart must require the sdp field"
            );
            assert!(
                multipart["properties"].get("session").is_some(),
                "{path} multipart must document the session field"
            );
        }
    }

    #[test]
    fn model_list_variants_are_documented() {
        let doc = doc();
        let schema = &doc["paths"]["/v1/models"]["get"]["responses"]["200"]["content"]["application/json"]["schema"];
        let refs: Vec<String> = schema["oneOf"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v["$ref"].as_str().map(str::to_owned))
            .collect();
        for name in ["ModelList", "ClaudeModelList", "CodexModelCatalog"] {
            assert!(
                refs.contains(&format!("#/components/schemas/{name}")),
                "model list must offer the {name} variant, got {refs:?}"
            );
        }
        let schemas = &doc["components"]["schemas"];
        let openai_required = required_keys(&schemas["ModelList"]);
        let claude_required = required_keys(&schemas["ClaudeModelList"]);
        let codex_required = required_keys(&schemas["CodexModelCatalog"]);
        assert!(openai_required.contains(&"object".to_owned()));
        assert!(
            !claude_required.contains(&"object".to_owned()),
            "the Anthropic catalog has no top-level object"
        );
        assert!(claude_required.contains(&"has_more".to_owned()));
        assert!(codex_required.contains(&"models".to_owned()));
        // Fixtures: a representative body of each variant carries exactly its required keys,
        // and the variants are disjoint so the oneOf is unambiguous.
        let openai = json!({ "object": "list", "data": [ { "id": "gpt-x", "object": "model" } ] });
        let claude = json!({
            "data": [ { "id": "claude-x", "type": "model" } ],
            "has_more": false,
            "first_id": "a",
            "last_id": "b"
        });
        let codex = json!({ "models": [ { "slug": "gpt-5.5" } ] });
        assert!(has_keys(&openai, &openai_required));
        assert!(has_keys(&claude, &claude_required));
        assert!(has_keys(&codex, &codex_required));
        assert!(openai.get("has_more").is_none() && claude.get("object").is_none());
    }

    #[test]
    fn relay_websocket_auth_is_optional() {
        let doc = doc();
        let security = &doc["paths"]["/v1/ws"]["get"]["security"];
        let alternatives = security.as_array().expect("security is an array");
        assert!(
            alternatives.iter().any(|a| a.as_object().is_some_and(|m| m.is_empty())),
            "ws-auth=false must be representable as an empty security alternative"
        );
        let names = scheme_names(security);
        for expected in [
            "clientBearer",
            "clientGoogApiKey",
            "clientApiKey",
            "clientQueryKey",
            "clientQueryAuthToken",
        ] {
            assert!(names.contains(&expected.to_owned()), "ws must still offer {expected}");
        }
    }

    #[test]
    fn dump_openapi_document() {
        let doc = doc();
        let text = serde_json::to_string_pretty(&doc).expect("document serializes");
        assert!(doc["paths"].as_object().is_some_and(|p| !p.is_empty()));
        if let Ok(path) = std::env::var("OPENAPI_DUMP") {
            std::fs::write(&path, &text).expect("write dump");
        }
    }
}
