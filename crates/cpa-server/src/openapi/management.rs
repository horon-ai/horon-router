//! OpenAPI 3.1 fragment for the fixed management HTTP API.
//!
//! [`extend`] merges this fragment into the document built by [`super::document`]: the
//! fixed v8 routes under `/v8/management`, the deprecated v0 spellings under
//! `/v0/management`, their schemas, the two management security schemes and two tags. It
//! never rewrites a general path, schema or scheme, and it registers no routes and
//! touches no handler.
//!
//! Scope limits, matching the plan:
//!
//! * Only the *fixed* management routes are described. The dynamic HTTP routes a plugin
//!   registers at runtime are not part of this contract.
//! * The static control-panel assets (`/ui`, `/management.html`, `/assets/*`,
//!   `/fonts/*`, `/favicon.svg`) are files, not API operations, so they are named in the
//!   tag text instead of being listed as paths.
//! * The management key is a different scheme from the general client keys; the OAuth
//!   callback routes take no key (they only require that management be available).
//! * A wildcard route (`/v8/management/config/{*path}`) is written as `{path}`, as the
//!   general document writes Gemini's `{*action}` as `{action}`.

use serde_json::{Map, Value, json};

const JSON: &str = "application/json";
const YAML: &str = "application/yaml";
const TEXT: &str = "text/plain";
const OCTET: &str = "application/octet-stream";
const MULTIPART: &str = "multipart/form-data";

const TAG_MANAGEMENT: &str = "Management";
const TAG_LEGACY: &str = "Management (legacy)";

/// Merges the management paths, schemas, security schemes and tags into `document`
/// without changing any general entry. `document` is the object [`super::document`]
/// builds, so its `paths`, `tags` and `components` entries are present and shaped.
pub(super) fn extend(document: &mut Value) {
    let paths = document["paths"]
        .as_object_mut()
        .expect("document() builds a `paths` object");
    for (path, item) in paths_list() {
        paths.insert(path, item);
    }
    let tags = document["tags"]
        .as_array_mut()
        .expect("document() builds a `tags` array");
    tags.extend(tags_list());
    let components = document["components"]
        .as_object_mut()
        .expect("document() builds a `components` object");
    let schemes = components["securitySchemes"]
        .as_object_mut()
        .expect("document() builds a `securitySchemes` object");
    for (name, scheme) in security_schemes() {
        schemes.insert(name.into(), scheme);
    }
    let schemas = components["schemas"]
        .as_object_mut()
        .expect("document() builds a `schemas` object");
    for (name, schema) in schemas_list() {
        schemas.insert(name.into(), schema);
    }
}

fn r(name: &str) -> Value {
    json!({ "$ref": format!("#/components/schemas/{name}") })
}

fn media_content(entries: &[(&str, Value)]) -> Value {
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
    json!({ "description": description, "content": media_content(entries) })
}

fn body(schema: Value) -> Value {
    body_media(&[(JSON, schema)])
}

fn body_media(entries: &[(&str, Value)]) -> Value {
    json!({ "required": true, "content": media_content(entries) })
}

fn optional_body(schema: Value) -> Value {
    json!({ "content": media_content(&[(JSON, schema)]) })
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

fn ok(description: &str, schema: &str) -> (&'static str, Value) {
    ("200", resp(description, &[(JSON, r(schema))]))
}

fn e400(description: &str) -> (&'static str, Value) {
    ("400", resp(description, &[(JSON, r("ManagementError"))]))
}

fn e404(description: &str) -> (&'static str, Value) {
    ("404", resp(description, &[(JSON, r("ManagementError"))]))
}

fn e409(description: &str) -> (&'static str, Value) {
    ("409", resp(description, &[(JSON, r("ManagementError"))]))
}

fn e422(description: &str) -> (&'static str, Value) {
    ("422", resp(description, &[(JSON, r("ManagementError"))]))
}

fn e500(description: &str) -> (&'static str, Value) {
    ("500", resp(description, &[(JSON, r("ManagementError"))]))
}

fn e501(description: &str) -> (&'static str, Value) {
    ("501", resp(description, &[(JSON, r("ManagementError"))]))
}

fn e502(description: &str) -> (&'static str, Value) {
    ("502", resp(description, &[(JSON, r("ManagementError"))]))
}

fn e503(description: &str) -> (&'static str, Value) {
    ("503", resp(description, &[(JSON, r("ManagementError"))]))
}

/// The statuses every guarded management route shares: a missing or wrong key, a refused
/// remote client or a ban, and a disabled management API (empty 404) or a missing
/// resource.
fn auth_responses() -> Vec<(&'static str, Value)> {
    vec![
        (
            "401",
            resp("Missing or invalid management key", &[(JSON, r("ManagementError"))]),
        ),
        (
            "403",
            resp(
                "Remote management is disabled, no management key is configured, or the client IP is banned after repeated failures",
                &[(JSON, r("ManagementError"))],
            ),
        ),
        (
            "404",
            resp(
                "The management API is disabled (empty body), or the resource was not found",
                &[(JSON, r("ManagementError"))],
            ),
        ),
    ]
}

fn op(
    (id, summary): (&str, &str),
    tag: &str,
    security: Value,
    params: Vec<Value>,
    request_body: Option<Value>,
    ok: Vec<(&'static str, Value)>,
    extra: Vec<(&'static str, Value)>,
) -> Value {
    let mut m = Map::new();
    m.insert("operationId".into(), Value::String(id.into()));
    m.insert("tags".into(), json!([tag]));
    m.insert("summary".into(), Value::String(summary.into()));
    m.insert("security".into(), security);
    if !params.is_empty() {
        m.insert("parameters".into(), Value::Array(params));
    }
    if let Some(request_body) = request_body {
        m.insert("requestBody".into(), request_body);
    }
    let mut rs = Map::new();
    for (status, response) in ok.into_iter().chain(auth_responses()).chain(extra) {
        rs.insert(status.into(), response);
    }
    m.insert("responses".into(), Value::Object(rs));
    Value::Object(m)
}

/// [`op`] for a v0 route: the legacy tag plus `deprecated: true`.
fn mgmt_op(
    legacy: bool,
    (id, summary): (&str, &str),
    security: Value,
    params: Vec<Value>,
    request_body: Option<Value>,
    ok: Vec<(&'static str, Value)>,
    extra: Vec<(&'static str, Value)>,
) -> Value {
    let tag = if legacy { TAG_LEGACY } else { TAG_MANAGEMENT };
    let built = op((id, summary), tag, security, params, request_body, ok, extra);
    if legacy { deprecated(built) } else { built }
}

fn deprecated(mut operation: Value) -> Value {
    if let Some(map) = operation.as_object_mut() {
        map.insert("deprecated".into(), Value::Bool(true));
    }
    operation
}

fn path_item(ops: Vec<(&'static str, Value)>) -> Value {
    let mut m = Map::new();
    for (method, operation) in ops {
        m.insert(method.into(), operation);
    }
    Value::Object(m)
}

/// The management key, in either location `management/access.rs` accepts (OR).
fn management_security() -> Value {
    json!([{ "managementBearer": [] }, { "managementKey": [] }])
}

/// The OAuth callback routes: no key, but management availability still gates them.
fn public_security() -> Value {
    json!([])
}

fn tags_list() -> Vec<Value> {
    vec![
        json!({
            "name": TAG_MANAGEMENT,
            "description": "Fixed v8 management API under `/v8/management`. Authentication \
        uses a management key (the `Authorization` header, `Bearer <key>` or the raw key, or \
        the `X-Management-Key` header) and is distinct from the client keys of the general \
        API. When no management key is configured the whole tree answers 404 with an empty \
        body. The static control panel (`/ui`, `/management.html`, `/assets/*`, `/fonts/*`, \
        `/favicon.svg`) is served as files, not as API operations, and the dynamic HTTP routes \
        a plugin registers at runtime are outside this contract.",
        }),
        json!({
            "name": TAG_LEGACY,
            "description": "Deprecated v0 spellings under `/v0/management`, Go's pre-v8 \
        tree. They share the v8 handlers and the same management authentication, so every \
        operation here is marked `deprecated`. The panel and dynamic-plugin limits of the v8 \
        tag apply here too.",
        }),
    ]
}

fn security_schemes() -> Vec<(&'static str, Value)> {
    vec![
        (
            "managementBearer",
            json!({
                "type": "http",
                "scheme": "bearer",
                "description": "Authorization header: `Bearer <key>` or the raw key value \
            (first value only). A non-`Bearer` Authorization value is used verbatim as the key.",
            }),
        ),
        (
            "managementKey",
            json!({
                "type": "apiKey",
                "in": "header",
                "name": "X-Management-Key",
                "description": "Used only when the Authorization header carries no value.",
            }),
        ),
    ]
}

fn paths_list() -> Vec<(String, Value)> {
    let mut items = Vec::new();
    items.extend(config_paths());
    items.extend(credentials_paths(
        "/v8/management",
        "credentials",
        "routing/model-definitions",
        false,
    ));
    items.extend(credentials_paths(
        "/v0/management",
        "auth-files",
        "model-definitions",
        true,
    ));
    items.extend(v8_paths());
    items.extend(v0_paths());
    items.extend(plugin_paths("/v8/management", false));
    items.extend(plugin_paths("/v0/management", true));
    items.extend(legacy_paths());
    items
}

fn config_paths() -> Vec<(String, Value)> {
    let read = || {
        resp(
            "The management config document as JSON. TURN ICE `username`/`credential` are omitted from JSON reads.",
            &[(JSON, r("ManagementConfig"))],
        )
    };
    let json_write_errors = || {
        vec![
            e400(
                "Invalid JSON body, an empty or comment-only body, a non-object root, an invalid path, or a read-only field",
            ),
            e422("The resulting config failed validation"),
            e500("The config file could not be read, decoded or written"),
        ]
    };
    let yaml_write_errors = || {
        vec![
            e400("Invalid YAML body or a v8 key group that fails Go's decode checks"),
            e422("The resulting config failed validation"),
            e500("The config file could not be written"),
        ]
    };
    let write_ok = |what: &str| resp(what, &[(JSON, r("ConfigWriteResult"))]);
    let node = |description: &str| resp(description, &[(JSON, r("ConfigNode"))]);
    let write = |id: &str, summary: &str| {
        mgmt_op(
            false,
            (id, summary),
            management_security(),
            vec![],
            Some(body(r("ManagementConfig"))),
            vec![("200", write_ok("Config written and reloaded"))],
            json_write_errors(),
        )
    };
    vec![
        (
            "/v8/management/config".into(),
            path_item(vec![
                (
                    "get",
                    mgmt_op(
                        false,
                        ("managementConfigGet", "Read the config document"),
                        management_security(),
                        vec![],
                        None,
                        vec![("200", read())],
                        vec![e500("read_failed")],
                    ),
                ),
                ("put", write("managementConfigPut", "Replace the config document")),
                (
                    "patch",
                    write("managementConfigPatch", "Merge into the config document"),
                ),
            ]),
        ),
        (
            "/v8/management/config.yaml".into(),
            path_item(vec![
                (
                    "get",
                    mgmt_op(
                        false,
                        ("managementConfigYamlGet", "Download the config file as YAML"),
                        management_security(),
                        vec![],
                        None,
                        vec![(
                            "200",
                            resp(
                                "The config file with comments preserved.",
                                &[(YAML, json!({ "type": "string" }))],
                            ),
                        )],
                        vec![e500("decode_failed")],
                    ),
                ),
                (
                    "put",
                    mgmt_op(
                        false,
                        ("managementConfigYamlPut", "Upload the config file as YAML"),
                        management_security(),
                        vec![],
                        Some(body_media(&[
                            (YAML, json!({ "type": "string" })),
                            (TEXT, json!({ "type": "string" })),
                        ])),
                        vec![("200", write_ok("Config written and reloaded"))],
                        yaml_write_errors(),
                    ),
                ),
            ]),
        ),
        (
            "/v8/management/config/".into(),
            path_item(vec![
                (
                    "get",
                    mgmt_op(
                        false,
                        (
                            "managementConfigRootGet",
                            "Read the whole config document (trailing slash)",
                        ),
                        management_security(),
                        vec![],
                        None,
                        vec![("200", node("The whole config document as JSON."))],
                        vec![e500("read_failed")],
                    ),
                ),
                (
                    "put",
                    mgmt_op(
                        false,
                        (
                            "managementConfigRootPut",
                            "Replace the whole config document (trailing slash)",
                        ),
                        management_security(),
                        vec![],
                        Some(body(r("ManagementConfig"))),
                        vec![("200", write_ok("Config written and reloaded"))],
                        json_write_errors(),
                    ),
                ),
                (
                    "patch",
                    mgmt_op(
                        false,
                        (
                            "managementConfigRootPatch",
                            "Merge into the whole config document (trailing slash)",
                        ),
                        management_security(),
                        vec![],
                        Some(body(r("ManagementConfig"))),
                        vec![("200", write_ok("Config patched and reloaded"))],
                        json_write_errors(),
                    ),
                ),
                (
                    "delete",
                    mgmt_op(
                        false,
                        ("managementConfigRootDelete", "Delete the root config (refused)"),
                        management_security(),
                        vec![],
                        None,
                        vec![],
                        vec![e400("cannot_delete_config")],
                    ),
                ),
            ]),
        ),
        (
            "/v8/management/config/{path}".into(),
            path_item(vec![
                (
                    "get",
                    mgmt_op(
                        false,
                        ("managementConfigNodeGet", "Read one config node"),
                        management_security(),
                        vec![path_param(
                            "path",
                            "The config path below the root, for example `server/port`.",
                        )],
                        None,
                        vec![("200", node("The selected config value as JSON."))],
                        vec![e404("The config path does not exist"), e500("read_failed")],
                    ),
                ),
                (
                    "put",
                    mgmt_op(
                        false,
                        ("managementConfigNodePut", "Replace one config node"),
                        management_security(),
                        vec![path_param("path", "The config path below the root.")],
                        Some(body(r("ManagementConfig"))),
                        vec![("200", write_ok("Config written and reloaded"))],
                        json_write_errors(),
                    ),
                ),
                (
                    "patch",
                    mgmt_op(
                        false,
                        ("managementConfigNodePatch", "Merge into one config node"),
                        management_security(),
                        vec![path_param("path", "The config path below the root.")],
                        Some(body(r("ManagementConfig"))),
                        vec![("200", write_ok("Config patched and reloaded"))],
                        json_write_errors(),
                    ),
                ),
                (
                    "delete",
                    mgmt_op(
                        false,
                        ("managementConfigNodeDelete", "Delete one config node"),
                        management_security(),
                        vec![path_param("path", "The config path below the root.")],
                        None,
                        vec![("200", write_ok("Config node deleted and reloaded"))],
                        vec![e400("cannot_delete_config"), e404("The config path does not exist")],
                    ),
                ),
            ]),
        ),
    ]
}

fn credentials_paths(base: &str, files: &str, definitions: &str, legacy: bool) -> Vec<(String, Value)> {
    let kind = if legacy { "AuthFiles" } else { "Credentials" };
    let family = format!("{base}/{files}");
    let mut items = Vec::new();
    items.push((
        family.clone(),
        path_item(vec![
            (
                "get",
                mgmt_op(
                    legacy,
                    (&format!("management{kind}List"), "List credentials"),
                    management_security(),
                    vec![
                        query_param("name", "Filter by credential ID or file name"),
                        query_param("auth_index", "Filter by auth index"),
                        query_param("page", "Page number (with `page_size`, enables pagination)"),
                        query_param("page_size", "Page size (with `page`, enables pagination)"),
                    ],
                    None,
                    vec![ok(
                        "The credential list. Without `page`/`page_size` every listable credential is returned.",
                        "CredentialList",
                    )],
                    vec![e400("`page` or `page_size` is not a positive integer")],
                ),
            ),
            (
                "post",
                mgmt_op(
                    legacy,
                    (
                        &format!("management{kind}Upload"),
                        "Upload one or more credential files",
                    ),
                    management_security(),
                    vec![query_param(
                        "name",
                        "Target file name; required unless the body is multipart",
                    )],
                    Some(body_media(&[
                        (JSON, r("CredentialFile")),
                        (MULTIPART, json!({ "type": "string", "format": "binary" })),
                    ])),
                    vec![ok("The upload result.", "CredentialMutation")],
                    vec![
                        e400("A missing or invalid `name`, a name that does not end in `.json`, or no multipart file"),
                        e500("The credential file could not be written"),
                    ],
                ),
            ),
            (
                "delete",
                mgmt_op(
                    legacy,
                    (&format!("management{kind}Delete"), "Delete credentials"),
                    management_security(),
                    vec![
                        query_param("name", "File name; repeatable"),
                        query_param("all", "`true`, `1` or `*` deletes every credential file"),
                    ],
                    Some(optional_body(r("CredentialDeleteRequest"))),
                    vec![ok("The deletion result.", "CredentialMutation")],
                    vec![
                        e400("No name was given"),
                        e404("The credential file does not exist"),
                        e500("The credential file could not be removed"),
                    ],
                ),
            ),
        ]),
    ));
    items.push((
        format!("{family}/models"),
        path_item(vec![(
            "get",
            mgmt_op(
                legacy,
                (&format!("management{kind}Models"), "List a credential's models"),
                management_security(),
                vec![query_param("name", "The credential ID or file name")],
                None,
                vec![ok("The models the credential registers.", "CredentialModels")],
                vec![e400("`name` is required")],
            ),
        )]),
    ));
    items.push((
        format!("{family}/download"),
        path_item(vec![(
            "get",
            mgmt_op(
                legacy,
                (&format!("management{kind}Download"), "Download a credential file"),
                management_security(),
                vec![query_param("name", "The file name, ending in `.json`")],
                None,
                vec![(
                    "200",
                    resp(
                        "The credential file bytes as a download.",
                        &[(JSON, json!({ "type": "string", "format": "binary" }))],
                    ),
                )],
                vec![
                    e400("`name` is invalid or does not end in `.json`"),
                    e404("The credential file does not exist"),
                    e500("The credential file could not be read"),
                ],
            ),
        )]),
    ));
    items.push((
        format!("{family}/status"),
        path_item(vec![(
            "patch",
            mgmt_op(
                legacy,
                (&format!("management{kind}Status"), "Enable or disable a credential"),
                management_security(),
                vec![],
                Some(body(r("CredentialStatusRequest"))),
                vec![ok("The new disabled state.", "CredentialStatus")],
                vec![
                    e400("`name` or `disabled` is missing"),
                    e404("The credential does not exist"),
                    e409("The credential changed since it was read"),
                    e500("The credential could not be updated"),
                ],
            ),
        )]),
    ));
    items.push((
        format!("{family}/fields"),
        path_item(vec![(
            "patch",
            mgmt_op(
                legacy,
                (&format!("management{kind}Fields"), "Patch credential metadata fields"),
                management_security(),
                vec![],
                Some(body(r("CredentialFieldsRequest"))),
                vec![("200", resp("The fields were updated.", &[(JSON, r("StatusOk"))]))],
                vec![
                    e400("A field name or value is invalid, or no field was given"),
                    e404("The credential does not exist"),
                    e409("The credential changed since it was read"),
                    e500("The credential could not be updated"),
                ],
            ),
        )]),
    ));
    items.push((
        format!("{family}/refresh"),
        path_item(vec![(
            "post",
            mgmt_op(
                legacy,
                (&format!("management{kind}Refresh"), "Refresh a credential token"),
                management_security(),
                vec![
                    query_param("name", "The credential ID or file name"),
                    query_param("auth_index", "The auth index"),
                    query_param("all", "`true` refreshes every refreshable credential"),
                ],
                Some(optional_body(r("CredentialRefreshRequest"))),
                vec![ok("The refresh result.", "CredentialRefresh")],
                vec![
                    e400("Neither `name` nor `all=true` was given, or the body is invalid"),
                    e404("The credential does not exist"),
                    e500("The upstream refresh failed"),
                ],
            ),
        )]),
    ));
    items.push((
        format!("{base}/{definitions}/{{channel}}"),
        path_item(vec![(
            "get",
            mgmt_op(
                legacy,
                (
                    &format!("management{kind}ModelDefinitions"),
                    "List a channel's static model definitions",
                ),
                management_security(),
                vec![path_param(
                    "channel",
                    "The provider channel, for example `claude` or `gemini`",
                )],
                None,
                vec![ok("The channel's static model definitions.", "ModelDefinitions")],
                vec![e400("Unknown channel")],
            ),
        )]),
    ));
    items
}

fn v8_paths() -> Vec<(String, Value)> {
    vec![
        (
            "/v8/management/server/latest-version".into(),
            path_item(vec![(
                "get",
                mgmt_op(
                    false,
                    ("managementServerLatestVersion", "Check the latest release"),
                    management_security(),
                    vec![],
                    None,
                    vec![ok("The latest release tag.", "LatestVersion")],
                    vec![
                        e502("Could not check the latest release"),
                        e503("Update checks are disabled by `CLIPROXY_NO_UPDATE_CHECK=1`"),
                    ],
                ),
            )]),
        ),
        (
            "/v8/management/requests/api-call".into(),
            path_item(vec![(
                "post",
                mgmt_op(
                    false,
                    ("managementRequestsApiCall", "Send a request through the proxy"),
                    management_security(),
                    vec![],
                    Some(body(r("ApiCallRequest"))),
                    vec![ok("The upstream response.", "ApiCallResponse")],
                    vec![
                        e400("The body, method, URL, proxy URL or `$TOKEN$` substitution is invalid"),
                        e502("The upstream request failed"),
                    ],
                ),
            )]),
        ),
        (
            "/v8/management/routing/cooldown/reset".into(),
            path_item(vec![(
                "post",
                mgmt_op(
                    false,
                    ("managementRoutingCooldownReset", "Reset a credential's cooldowns"),
                    management_security(),
                    vec![],
                    Some(body(r("AuthIndexRequest"))),
                    vec![ok("The reset result.", "CooldownReset")],
                    vec![
                        e400("`auth_index` is required"),
                        e404("No credential matches the auth index"),
                    ],
                ),
            )]),
        ),
        (
            "/v8/management/observability/usage/api-keys".into(),
            path_item(vec![(
                "get",
                mgmt_op(
                    false,
                    ("managementObservabilityApiKeyUsage", "API-key usage by provider"),
                    management_security(),
                    vec![],
                    None,
                    vec![ok("Usage counters grouped by provider and key.", "ApiKeyUsage")],
                    vec![],
                ),
            )]),
        ),
        (
            "/v8/management/observability/usage/queue".into(),
            path_item(vec![(
                "get",
                mgmt_op(
                    false,
                    ("managementObservabilityUsageQueue", "Pop queued usage records"),
                    management_security(),
                    vec![query_param("count", "How many records to pop (default 1)")],
                    None,
                    vec![ok("The popped usage records.", "UsageQueue")],
                    vec![e400("`count` is not a positive integer")],
                ),
            )]),
        ),
        (
            "/v8/management/observability/logs".into(),
            path_item(vec![
                (
                    "get",
                    mgmt_op(
                        false,
                        ("managementObservabilityLogsGet", "Read log lines"),
                        management_security(),
                        vec![
                            query_param("cursor", "Continue from a previous `next-cursor`"),
                            query_param("after", "Only lines newer than this Unix-nanosecond timestamp"),
                            query_param("limit", "Maximum number of lines"),
                        ],
                        None,
                        vec![ok("A page of log lines.", "LogsPage")],
                        vec![e400("`limit` is not a positive integer, or file logging is disabled")],
                    ),
                ),
                (
                    "delete",
                    mgmt_op(
                        false,
                        ("managementObservabilityLogsDelete", "Delete the log files"),
                        management_security(),
                        vec![],
                        None,
                        vec![("200", resp("The log files were deleted.", &[(JSON, r("StatusOk"))]))],
                        vec![
                            e400("File logging is disabled"),
                            e404("The log directory does not exist"),
                            e500("The log directory could not be read"),
                        ],
                    ),
                ),
            ]),
        ),
        (
            "/v8/management/observability/logs/errors".into(),
            path_item(vec![(
                "get",
                mgmt_op(
                    false,
                    ("managementObservabilityErrorLogs", "List request error logs"),
                    management_security(),
                    vec![],
                    None,
                    vec![ok("The request error log files, newest first.", "ErrorLogs")],
                    vec![e500("The log directory could not be read")],
                ),
            )]),
        ),
        (
            "/v8/management/observability/logs/errors/{name}".into(),
            path_item(vec![(
                "get",
                mgmt_op(
                    false,
                    (
                        "managementObservabilityErrorLogDownload",
                        "Download a request error log",
                    ),
                    management_security(),
                    vec![path_param("name", "The `error-*.log` file name")],
                    None,
                    vec![(
                        "200",
                        resp(
                            "The log file bytes as a download.",
                            &[
                                (OCTET, json!({ "type": "string", "format": "binary" })),
                                (TEXT, json!({ "type": "string" })),
                            ],
                        ),
                    )],
                    vec![
                        e400("The log file name is invalid"),
                        e404("The log file does not exist"),
                        e500("The log file could not be read"),
                    ],
                ),
            )]),
        ),
        (
            "/v8/management/observability/logs/requests/{id}".into(),
            path_item(vec![(
                "get",
                mgmt_op(
                    false,
                    (
                        "managementObservabilityRequestLogDownload",
                        "Download a request log by ID",
                    ),
                    management_security(),
                    vec![path_param(
                        "id",
                        "The request ID (its last 8 characters select the file)",
                    )],
                    None,
                    vec![(
                        "200",
                        resp(
                            "The request log file bytes as a download.",
                            &[
                                (OCTET, json!({ "type": "string", "format": "binary" })),
                                (TEXT, json!({ "type": "string" })),
                            ],
                        ),
                    )],
                    vec![
                        e400("The request ID is missing or invalid"),
                        e404("No log file matches the request ID"),
                        e500("The log directory could not be read"),
                    ],
                ),
            )]),
        ),
        (
            "/v8/management/oauth/auth-url".into(),
            path_item(vec![(
                "get",
                mgmt_op(
                    false,
                    ("managementOAuthAuthUrl", "Start a provider login"),
                    management_security(),
                    vec![
                        query_param("provider", "The provider to authenticate"),
                        query_param("is_webui", "`1`/`true`/`yes`/`on` for the web UI flow"),
                        query_param("domain", "Kimi domain override"),
                        query_param("channel", "Kimi channel alias for `domain`"),
                    ],
                    None,
                    vec![ok("The login URL and its state.", "OAuthStart")],
                    vec![e400("`provider` is required"), e404("Unknown provider")],
                ),
            )]),
        ),
        (
            "/v8/management/oauth/status".into(),
            path_item(vec![(
                "get",
                mgmt_op(
                    false,
                    ("managementOAuthStatus", "Poll a login session"),
                    management_security(),
                    vec![query_param("state", "The login state")],
                    None,
                    vec![ok("The session status.", "OAuthStatus")],
                    vec![e400("The state is invalid")],
                ),
            )]),
        ),
        (
            "/v8/management/oauth/session".into(),
            path_item(vec![(
                "delete",
                mgmt_op(
                    false,
                    ("managementOAuthSession", "Cancel a login session"),
                    management_security(),
                    vec![query_param("state", "The login state")],
                    None,
                    vec![ok("Whether a session was cancelled.", "OAuthCancel")],
                    vec![e400("The state is missing or invalid")],
                ),
            )]),
        ),
        (
            "/v8/management/oauth/import".into(),
            path_item(vec![(
                "post",
                mgmt_op(
                    false,
                    ("managementOAuthImport", "Import an OAuth credential"),
                    management_security(),
                    vec![query_param("provider", "The provider to import")],
                    None,
                    vec![],
                    vec![e400("`provider` is required"), e404("The provider has no importer")],
                ),
            )]),
        ),
        (
            "/v8/management/oauth/callback".into(),
            path_item(vec![
                (
                    "get",
                    mgmt_op(
                        false,
                        ("managementOAuthCallbackGet", "OAuth redirect callback (no key)"),
                        public_security(),
                        vec![
                            query_param("provider", "The provider"),
                            query_param("code", "Authorization code"),
                            query_param("state", "Login state"),
                            query_param("error", "Error code, else `error_description`"),
                        ],
                        None,
                        vec![("200", resp("The callback was accepted.", &[(JSON, r("StatusOk"))]))],
                        vec![
                            e400("The state, provider or code is missing or invalid"),
                            e404("The login state is unknown or expired"),
                            e409("The flow is already completed or has failed"),
                        ],
                    ),
                ),
                (
                    "post",
                    mgmt_op(
                        false,
                        ("managementOAuthCallbackPost", "OAuth redirect callback (no key)"),
                        public_security(),
                        vec![],
                        Some(body(r("OAuthCallbackRequest"))),
                        vec![("200", resp("The callback was accepted.", &[(JSON, r("StatusOk"))]))],
                        vec![
                            e400("The body is invalid, or the state, provider or code is missing or invalid"),
                            e404("The login state is unknown or expired"),
                            e409("The flow is already completed or has failed"),
                        ],
                    ),
                ),
            ]),
        ),
        (
            "/v8/management/quota/fetch".into(),
            path_item(vec![(
                "post",
                mgmt_op(
                    false,
                    ("managementQuotaFetch", "Fetch a credential's quota"),
                    management_security(),
                    vec![],
                    Some(body(r("QuotaRequest"))),
                    vec![ok("The quota snapshot.", "QuotaResponse")],
                    vec![
                        e400("`auth_index` is required or the body is invalid"),
                        e404("The credential does not exist"),
                        e501("No quota provider is available for the credential"),
                        e502("The quota provider failed"),
                    ],
                ),
            )]),
        ),
    ]
}

fn v0_paths() -> Vec<(String, Value)> {
    vec![
        (
            "/v0/management/config.yaml".into(),
            path_item(vec![
                (
                    "get",
                    mgmt_op(
                        true,
                        ("managementLegacyConfigYamlGet", "Download the config file as YAML"),
                        management_security(),
                        vec![],
                        None,
                        vec![(
                            "200",
                            resp("The config file bytes.", &[(YAML, json!({ "type": "string" }))]),
                        )],
                        vec![e404("The config file does not exist")],
                    ),
                ),
                (
                    "put",
                    mgmt_op(
                        true,
                        ("managementLegacyConfigYamlPut", "Upload the config file as YAML"),
                        management_security(),
                        vec![],
                        Some(body_media(&[
                            (YAML, json!({ "type": "string" })),
                            (TEXT, json!({ "type": "string" })),
                        ])),
                        vec![ok("The config was written and reloaded.", "ConfigYamlUploadResult")],
                        vec![
                            e400("The YAML body or a v8 key group is invalid"),
                            e422("The resulting config failed validation"),
                            e500("The config file could not be written or reloaded"),
                        ],
                    ),
                ),
            ]),
        ),
        (
            "/v0/management/config".into(),
            path_item(vec![(
                "get",
                mgmt_op(
                    true,
                    ("managementLegacyConfigGet", "Read the config (Go's runtime view)"),
                    management_security(),
                    vec![],
                    None,
                    vec![ok("The runtime config view as JSON.", "ManagementConfig")],
                    vec![],
                ),
            )]),
        ),
        (
            "/v0/management/latest-version".into(),
            path_item(vec![(
                "get",
                mgmt_op(
                    true,
                    ("managementLegacyLatestVersion", "Check the latest release"),
                    management_security(),
                    vec![],
                    None,
                    vec![ok("The latest release tag.", "LatestVersion")],
                    vec![
                        e502("Could not check the latest release"),
                        e503("Update checks are disabled by `CLIPROXY_NO_UPDATE_CHECK=1`"),
                    ],
                ),
            )]),
        ),
        (
            "/v0/management/api-call".into(),
            path_item(vec![(
                "post",
                mgmt_op(
                    true,
                    ("managementLegacyApiCall", "Send a request through the proxy"),
                    management_security(),
                    vec![],
                    Some(body(r("ApiCallRequest"))),
                    vec![ok("The upstream response.", "ApiCallResponse")],
                    vec![
                        e400("The body, method, URL, proxy URL or `$TOKEN$` substitution is invalid"),
                        e502("The upstream request failed"),
                    ],
                ),
            )]),
        ),
        (
            "/v0/management/reset-quota".into(),
            path_item(vec![(
                "post",
                mgmt_op(
                    true,
                    ("managementLegacyResetQuota", "Reset a credential's cooldowns"),
                    management_security(),
                    vec![],
                    Some(body(r("AuthIndexRequest"))),
                    vec![ok("The reset result.", "CooldownReset")],
                    vec![
                        e400("`auth_index` is required"),
                        e404("No credential matches the auth index"),
                    ],
                ),
            )]),
        ),
        (
            "/v0/management/api-key-usage".into(),
            path_item(vec![(
                "get",
                mgmt_op(
                    true,
                    ("managementLegacyApiKeyUsage", "API-key usage by provider"),
                    management_security(),
                    vec![],
                    None,
                    vec![ok("Usage counters grouped by provider and key.", "ApiKeyUsage")],
                    vec![],
                ),
            )]),
        ),
        (
            "/v0/management/usage-queue".into(),
            path_item(vec![(
                "get",
                mgmt_op(
                    true,
                    ("managementLegacyUsageQueue", "Pop queued usage records"),
                    management_security(),
                    vec![query_param("count", "How many records to pop (default 1)")],
                    None,
                    vec![ok("The popped usage records.", "UsageQueue")],
                    vec![e400("`count` is not a positive integer")],
                ),
            )]),
        ),
        (
            "/v0/management/get-auth-status".into(),
            path_item(vec![(
                "get",
                mgmt_op(
                    true,
                    ("managementLegacyAuthStatus", "Poll a login session"),
                    management_security(),
                    vec![query_param("state", "The login state")],
                    None,
                    vec![ok("The session status.", "OAuthStatus")],
                    vec![e400("The state is invalid")],
                ),
            )]),
        ),
        (
            "/v0/management/oauth-session".into(),
            path_item(vec![(
                "delete",
                mgmt_op(
                    true,
                    ("managementLegacyOAuthSession", "Cancel a login session"),
                    management_security(),
                    vec![query_param("state", "The login state")],
                    None,
                    vec![ok("Whether a session was cancelled.", "OAuthCancel")],
                    vec![e400("The state is missing or invalid")],
                ),
            )]),
        ),
        (
            "/v0/management/oauth-callback".into(),
            path_item(vec![
                (
                    "get",
                    mgmt_op(
                        true,
                        ("managementLegacyOAuthCallbackGet", "OAuth redirect callback (no key)"),
                        public_security(),
                        vec![
                            query_param("provider", "The provider"),
                            query_param("code", "Authorization code"),
                            query_param("state", "Login state"),
                            query_param("error", "Error code, else `error_description`"),
                        ],
                        None,
                        vec![("200", resp("The callback was accepted.", &[(JSON, r("StatusOk"))]))],
                        vec![
                            e400("The state, provider or code is missing or invalid"),
                            e404("The login state is unknown or expired"),
                            e409("The flow is already completed or has failed"),
                        ],
                    ),
                ),
                (
                    "post",
                    mgmt_op(
                        true,
                        ("managementLegacyOAuthCallbackPost", "OAuth redirect callback (no key)"),
                        public_security(),
                        vec![],
                        Some(body(r("OAuthCallbackRequest"))),
                        vec![("200", resp("The callback was accepted.", &[(JSON, r("StatusOk"))]))],
                        vec![
                            e400("The body is invalid, or the state, provider or code is missing or invalid"),
                            e404("The login state is unknown or expired"),
                            e409("The flow is already completed or has failed"),
                        ],
                    ),
                ),
            ]),
        ),
        (
            "/v0/management/quota/providers".into(),
            path_item(vec![(
                "get",
                mgmt_op(
                    true,
                    ("managementLegacyQuotaProviders", "List quota providers"),
                    management_security(),
                    vec![],
                    None,
                    vec![ok("The registered quota providers.", "QuotaProviders")],
                    vec![],
                ),
            )]),
        ),
        (
            "/v0/management/quota/fetch".into(),
            path_item(vec![(
                "post",
                mgmt_op(
                    true,
                    ("managementLegacyQuotaFetch", "Fetch a credential's quota"),
                    management_security(),
                    vec![],
                    Some(body(r("QuotaRequest"))),
                    vec![ok("The quota snapshot.", "QuotaResponse")],
                    vec![
                        e400("`auth_index` is required or the body is invalid"),
                        e404("The credential does not exist"),
                        e501("No quota provider is available for the credential"),
                        e502("The quota provider failed"),
                    ],
                ),
            )]),
        ),
        (
            "/v0/management/quota/reset".into(),
            path_item(vec![(
                "post",
                mgmt_op(
                    true,
                    ("managementLegacyQuotaReset", "Reset a credential's quota"),
                    management_security(),
                    vec![],
                    Some(body(r("QuotaRequest"))),
                    vec![ok("The quota was reset.", "CooldownReset")],
                    vec![
                        e400("`auth_index` is required or the body is invalid"),
                        e404("The credential does not exist"),
                        e501("No quota provider is available to reset"),
                        e502("The quota provider failed"),
                    ],
                ),
            )]),
        ),
        (
            "/v0/management/vertex/import".into(),
            path_item(vec![(
                "post",
                mgmt_op(
                    true,
                    ("managementLegacyVertexImport", "Import a Vertex credential"),
                    management_security(),
                    vec![query_param("provider", "Forced to `vertex` by this route")],
                    None,
                    vec![],
                    vec![e400("`provider` is required"), e404("The provider has no importer")],
                ),
            )]),
        ),
        (
            "/v0/management/logs".into(),
            path_item(vec![
                (
                    "get",
                    mgmt_op(
                        true,
                        ("managementLegacyLogsGet", "Read log lines"),
                        management_security(),
                        vec![
                            query_param("cursor", "Continue from a previous `next-cursor`"),
                            query_param("after", "Only lines newer than this Unix-nanosecond timestamp"),
                            query_param("limit", "Maximum number of lines"),
                        ],
                        None,
                        vec![ok("A page of log lines.", "LogsPage")],
                        vec![e400("`limit` is not a positive integer, or file logging is disabled")],
                    ),
                ),
                (
                    "delete",
                    mgmt_op(
                        true,
                        ("managementLegacyLogsDelete", "Delete the log files"),
                        management_security(),
                        vec![],
                        None,
                        vec![("200", resp("The log files were deleted.", &[(JSON, r("StatusOk"))]))],
                        vec![
                            e400("File logging is disabled"),
                            e404("The log directory does not exist"),
                            e500("The log directory could not be read"),
                        ],
                    ),
                ),
            ]),
        ),
        (
            "/v0/management/request-error-logs".into(),
            path_item(vec![(
                "get",
                mgmt_op(
                    true,
                    ("managementLegacyErrorLogs", "List request error logs"),
                    management_security(),
                    vec![],
                    None,
                    vec![ok("The request error log files, newest first.", "ErrorLogs")],
                    vec![e500("The log directory could not be read")],
                ),
            )]),
        ),
        (
            "/v0/management/request-error-logs/{name}".into(),
            path_item(vec![(
                "get",
                mgmt_op(
                    true,
                    ("managementLegacyErrorLogDownload", "Download a request error log"),
                    management_security(),
                    vec![path_param("name", "The `error-*.log` file name")],
                    None,
                    vec![(
                        "200",
                        resp(
                            "The log file bytes as a download.",
                            &[
                                (OCTET, json!({ "type": "string", "format": "binary" })),
                                (TEXT, json!({ "type": "string" })),
                            ],
                        ),
                    )],
                    vec![
                        e400("The log file name is invalid"),
                        e404("The log file does not exist"),
                        e500("The log file could not be read"),
                    ],
                ),
            )]),
        ),
        (
            "/v0/management/request-log-by-id/{id}".into(),
            path_item(vec![(
                "get",
                mgmt_op(
                    true,
                    ("managementLegacyRequestLogDownload", "Download a request log by ID"),
                    management_security(),
                    vec![path_param(
                        "id",
                        "The request ID (its last 8 characters select the file)",
                    )],
                    None,
                    vec![(
                        "200",
                        resp(
                            "The request log file bytes as a download.",
                            &[
                                (OCTET, json!({ "type": "string", "format": "binary" })),
                                (TEXT, json!({ "type": "string" })),
                            ],
                        ),
                    )],
                    vec![
                        e400("The request ID is missing or invalid"),
                        e404("No log file matches the request ID"),
                        e500("The log directory could not be read"),
                    ],
                ),
            )]),
        ),
    ]
}

fn plugin_paths(base: &str, legacy: bool) -> Vec<(String, Value)> {
    let mut items = vec![
        (
            format!("{base}/plugins"),
            path_item(vec![(
                "get",
                mgmt_op(
                    legacy,
                    (
                        if legacy {
                            "managementLegacyPluginsList"
                        } else {
                            "managementPluginsList"
                        },
                        "List plugins",
                    ),
                    management_security(),
                    vec![],
                    None,
                    vec![ok("The discovered, configured and registered plugins.", "PluginsList")],
                    vec![e500("The plugins directory could not be read")],
                ),
            )]),
        ),
        (
            format!("{base}/plugins/{{id}}"),
            path_item(vec![(
                "delete",
                mgmt_op(
                    legacy,
                    (
                        if legacy {
                            "managementLegacyPluginsDelete"
                        } else {
                            "managementPluginsDelete"
                        },
                        "Delete a plugin",
                    ),
                    management_security(),
                    vec![path_param("id", "The plugin ID")],
                    None,
                    vec![(
                        "200",
                        resp("The plugin file and config were removed.", &[(JSON, r("PluginDelete"))]),
                    )],
                    vec![
                        e400("The plugin ID is invalid"),
                        e404("The plugin does not exist"),
                        e500("The plugin could not be removed"),
                    ],
                ),
            )]),
        ),
        (
            format!("{base}/plugins/{{id}}/quota"),
            path_item(vec![
                (
                    "get",
                    mgmt_op(
                        legacy,
                        (
                            if legacy {
                                "managementLegacyPluginQuotaGet"
                            } else {
                                "managementPluginQuotaGet"
                            },
                            "Read a plugin-managed quota",
                        ),
                        management_security(),
                        vec![
                            path_param("id", "The plugin ID"),
                            query_param("auth_index", "The credential's auth index (also `authIndex`)"),
                        ],
                        None,
                        vec![ok("The quota snapshot.", "QuotaResponse")],
                        vec![
                            e400("`auth_index` is required"),
                            e404("The credential or quota provider was not found"),
                            e502("The quota provider failed"),
                        ],
                    ),
                ),
                (
                    "post",
                    mgmt_op(
                        legacy,
                        (
                            if legacy {
                                "managementLegacyPluginQuotaPost"
                            } else {
                                "managementPluginQuotaPost"
                            },
                            "Fetch a plugin-managed quota",
                        ),
                        management_security(),
                        vec![path_param("id", "The plugin ID")],
                        Some(body(r("QuotaRequest"))),
                        vec![ok("The quota snapshot.", "QuotaResponse")],
                        vec![
                            e400("`auth_index` is required or the body is invalid"),
                            e404("The credential or quota provider was not found"),
                            e502("The quota provider failed"),
                        ],
                    ),
                ),
                (
                    "delete",
                    mgmt_op(
                        legacy,
                        (
                            if legacy {
                                "managementLegacyPluginQuotaDelete"
                            } else {
                                "managementPluginQuotaDelete"
                            },
                            "Reset a plugin-managed quota",
                        ),
                        management_security(),
                        vec![
                            path_param("id", "The plugin ID"),
                            query_param("auth_index", "The credential's auth index (also `authIndex`)"),
                        ],
                        Some(body(r("QuotaRequest"))),
                        vec![ok("The quota was reset.", "CooldownReset")],
                        vec![
                            e400("`auth_index` is required"),
                            e404("The credential or quota provider was not found"),
                            e502("The quota provider failed"),
                        ],
                    ),
                ),
            ]),
        ),
    ];
    if legacy {
        items.push((
            format!("{base}/plugins/{{id}}/quota/reset"),
            path_item(vec![(
                "post",
                mgmt_op(
                    true,
                    ("managementLegacyPluginQuotaReset", "Reset a plugin-managed quota"),
                    management_security(),
                    vec![
                        path_param("id", "The plugin ID"),
                        query_param("auth_index", "The credential's auth index (also `authIndex`)"),
                    ],
                    Some(body(r("QuotaRequest"))),
                    vec![ok("The quota was reset.", "CooldownReset")],
                    vec![
                        e400("`auth_index` is required"),
                        e404("The credential or quota provider was not found"),
                        e502("The quota provider failed"),
                    ],
                ),
            )]),
        ));
        items.push((
            format!("{base}/plugins/{{id}}/enabled"),
            path_item(vec![(
                "patch",
                mgmt_op(
                    true,
                    ("managementLegacyPluginEnabled", "Enable or disable a plugin"),
                    management_security(),
                    vec![path_param("id", "The plugin ID")],
                    Some(body(r("PluginEnabledRequest"))),
                    vec![("200", resp("The plugin was updated.", &[(JSON, r("StatusOk"))]))],
                    vec![
                        e400("The plugin ID is invalid or `enabled` is missing"),
                        e404("The plugin does not exist"),
                        e500("The config could not be saved"),
                    ],
                ),
            )]),
        ));
        items.push((
            format!("{base}/plugins/{{id}}/config"),
            path_item(vec![
                (
                    "get",
                    mgmt_op(
                        true,
                        ("managementLegacyPluginConfigGet", "Read a plugin's config"),
                        management_security(),
                        vec![path_param("id", "The plugin ID")],
                        None,
                        vec![ok("The plugin's config subtree.", "PluginConfig")],
                        vec![e400("The plugin ID is invalid"), e404("The plugin does not exist")],
                    ),
                ),
                (
                    "put",
                    mgmt_op(
                        true,
                        ("managementLegacyPluginConfigPut", "Replace a plugin's config"),
                        management_security(),
                        vec![path_param("id", "The plugin ID")],
                        Some(body(r("PluginConfig"))),
                        vec![("200", resp("The plugin config was replaced.", &[(JSON, r("StatusOk"))]))],
                        vec![
                            e400("The plugin ID or body is invalid"),
                            e404("The plugin does not exist"),
                            e500("The config could not be saved"),
                        ],
                    ),
                ),
                (
                    "patch",
                    mgmt_op(
                        true,
                        ("managementLegacyPluginConfigPatch", "Merge into a plugin's config"),
                        management_security(),
                        vec![path_param("id", "The plugin ID")],
                        Some(body(r("PluginConfig"))),
                        vec![("200", resp("The plugin config was patched.", &[(JSON, r("StatusOk"))]))],
                        vec![
                            e400("The plugin ID or body is invalid"),
                            e404("The plugin does not exist"),
                            e500("The config could not be saved"),
                        ],
                    ),
                ),
            ]),
        ));
    } else {
        items.push((
            format!("{base}/plugins/store"),
            path_item(vec![
                (
                    "get",
                    mgmt_op(
                        false,
                        ("managementPluginStoreList", "Browse the plugin store"),
                        management_security(),
                        vec![],
                        None,
                        vec![ok("The plugin store catalog and its sources.", "PluginStoreList")],
                        vec![e502("The plugin store registry could not be reached")],
                    ),
                ),
                (
                    "delete",
                    mgmt_op(
                        false,
                        ("managementPluginStoreDelete", "Delete the plugin with the ID `store`"),
                        management_security(),
                        vec![],
                        None,
                        vec![(
                            "200",
                            resp("The plugin file and config were removed.", &[(JSON, r("PluginDelete"))]),
                        )],
                        vec![
                            e400("The plugin ID is invalid"),
                            e404("The plugin does not exist"),
                            e500("The plugin could not be removed"),
                        ],
                    ),
                ),
            ]),
        ));
        items.push((
            format!("{base}/plugins/store/{{id}}/install"),
            path_item(vec![(
                "post",
                mgmt_op(
                    false,
                    ("managementPluginStoreInstall", "Install a plugin from the store"),
                    management_security(),
                    vec![
                        path_param("id", "The plugin ID"),
                        query_param("version", "The version to install"),
                        query_param("source", "The store source ID"),
                    ],
                    Some(body(r("PluginInstallRequest"))),
                    vec![ok("The install result.", "PluginInstall")],
                    vec![
                        e400("The plugin ID, version or body is invalid"),
                        e404("The plugin or source was not found"),
                        e409("A loaded plugin cannot be overwritten while the server runs"),
                        e500("The plugin was installed but the config could not be saved"),
                        e502("The plugin store registry or manifest failed"),
                    ],
                ),
            )]),
        ));
    }
    if legacy {
        items.push((
            format!("{base}/plugin-store"),
            path_item(vec![(
                "get",
                mgmt_op(
                    true,
                    ("managementLegacyPluginStoreList", "Browse the plugin store"),
                    management_security(),
                    vec![],
                    None,
                    vec![ok("The plugin store catalog and its sources.", "PluginStoreList")],
                    vec![e502("The plugin store registry could not be reached")],
                ),
            )]),
        ));
        items.push((
            format!("{base}/plugin-store/{{id}}/install"),
            path_item(vec![(
                "post",
                mgmt_op(
                    true,
                    ("managementLegacyPluginStoreInstall", "Install a plugin from the store"),
                    management_security(),
                    vec![
                        path_param("id", "The plugin ID"),
                        query_param("version", "The version to install"),
                        query_param("source", "The store source ID"),
                    ],
                    Some(body(r("PluginInstallRequest"))),
                    vec![ok("The install result.", "PluginInstall")],
                    vec![
                        e400("The plugin ID, version or body is invalid"),
                        e404("The plugin or source was not found"),
                        e409("A loaded plugin cannot be overwritten while the server runs"),
                        e500("The plugin was installed but the config could not be saved"),
                        e502("The plugin store registry or manifest failed"),
                    ],
                ),
            )]),
        ));
    }
    items
}

fn legacy_paths() -> Vec<(String, Value)> {
    let mut items = Vec::new();
    for &route in &crate::management::legacy::FIELD_ROUTES {
        let id = camel(route);
        let mut ops = vec![
            (
                "get",
                mgmt_op(
                    true,
                    (&format!("managementLegacyField{id}Get"), "Read a config value"),
                    management_security(),
                    vec![],
                    None,
                    vec![ok("The config value under its field key.", "FieldValue")],
                    vec![],
                ),
            ),
            (
                "put",
                mgmt_op(
                    true,
                    (&format!("managementLegacyField{id}Put"), "Write a config value"),
                    management_security(),
                    vec![],
                    Some(body(r("FieldWriteBody"))),
                    vec![("200", resp("The value was written.", &[(JSON, r("StatusOk"))]))],
                    vec![
                        e400("The body is invalid or the value is out of range"),
                        e500("The config could not be saved"),
                    ],
                ),
            ),
            (
                "patch",
                mgmt_op(
                    true,
                    (&format!("managementLegacyField{id}Patch"), "Write a config value"),
                    management_security(),
                    vec![],
                    Some(body(r("FieldWriteBody"))),
                    vec![("200", resp("The value was written.", &[(JSON, r("StatusOk"))]))],
                    vec![
                        e400("The body is invalid or the value is out of range"),
                        e500("The config could not be saved"),
                    ],
                ),
            ),
        ];
        if route == "proxy-url" {
            ops.push((
                "delete",
                mgmt_op(
                    true,
                    (&format!("managementLegacyField{id}Delete"), "Clear a config value"),
                    management_security(),
                    vec![],
                    None,
                    vec![("200", resp("The value was cleared.", &[(JSON, r("StatusOk"))]))],
                    vec![e500("The config could not be saved")],
                ),
            ));
        }
        items.push((format!("/v0/management/{route}"), path_item(ops)));
    }
    items.push((
        "/v0/management/api-keys".into(),
        path_item(vec![
            (
                "get",
                mgmt_op(
                    true,
                    ("managementLegacyApiKeysGet", "Read the client API keys"),
                    management_security(),
                    vec![],
                    None,
                    vec![ok("The configured client keys.", "ApiKeys")],
                    vec![],
                ),
            ),
            (
                "put",
                mgmt_op(
                    true,
                    ("managementLegacyApiKeysPut", "Replace the client API keys"),
                    management_security(),
                    vec![],
                    Some(body(r("ListWriteBody"))),
                    vec![("200", resp("The keys were written.", &[(JSON, r("StatusOk"))]))],
                    vec![e400("The body is invalid"), e500("The config could not be saved")],
                ),
            ),
            (
                "patch",
                mgmt_op(
                    true,
                    ("managementLegacyApiKeysPatch", "Patch the client API keys"),
                    management_security(),
                    vec![],
                    Some(body(r("ListPatchBody"))),
                    vec![("200", resp("The keys were patched.", &[(JSON, r("StatusOk"))]))],
                    vec![
                        e400("`old`/`new` or `index`/`value` is missing"),
                        e500("The config could not be saved"),
                    ],
                ),
            ),
            (
                "delete",
                mgmt_op(
                    true,
                    ("managementLegacyApiKeysDelete", "Delete a client API key"),
                    management_security(),
                    vec![
                        query_param("index", "The key's index"),
                        query_param("value", "The key value to remove"),
                    ],
                    None,
                    vec![("200", resp("The key was removed.", &[(JSON, r("StatusOk"))]))],
                    vec![
                        e400("Neither `index` nor `value` was given"),
                        e500("The config could not be saved"),
                    ],
                ),
            ),
        ]),
    ));
    for &route in &crate::management::legacy::LIST_ROUTES {
        let id = camel(route);
        items.push((
            format!("/v0/management/{route}"),
            path_item(vec![
                (
                    "get",
                    mgmt_op(
                        true,
                        (
                            &format!("managementLegacyList{id}Get"),
                            "Read a provider key list or OAuth map",
                        ),
                        management_security(),
                        vec![],
                        None,
                        vec![ok("The list or map under its route key.", "ListValue")],
                        vec![],
                    ),
                ),
                (
                    "put",
                    mgmt_op(
                        true,
                        (
                            &format!("managementLegacyList{id}Put"),
                            "Replace a provider key list or OAuth map",
                        ),
                        management_security(),
                        vec![],
                        Some(body(r("ListWriteBody"))),
                        vec![("200", resp("The list was written.", &[(JSON, r("StatusOk"))]))],
                        vec![
                            e400("The body is invalid"),
                            e422("The resulting config failed validation"),
                            e500("The config could not be saved"),
                        ],
                    ),
                ),
                (
                    "patch",
                    mgmt_op(
                        true,
                        (
                            &format!("managementLegacyList{id}Patch"),
                            "Patch a provider key list or OAuth map",
                        ),
                        management_security(),
                        vec![],
                        Some(body(r("ListPatchBody"))),
                        vec![("200", resp("The list was patched.", &[(JSON, r("StatusOk"))]))],
                        vec![
                            e400("The body is invalid"),
                            e422("The resulting config failed validation"),
                            e500("The config could not be saved"),
                        ],
                    ),
                ),
                (
                    "delete",
                    mgmt_op(
                        true,
                        (
                            &format!("managementLegacyList{id}Delete"),
                            "Delete from a provider key list or OAuth map",
                        ),
                        management_security(),
                        vec![],
                        Some(body(r("ListPatchBody"))),
                        vec![("200", resp("The entry was removed.", &[(JSON, r("StatusOk"))]))],
                        vec![
                            e400("The body is invalid"),
                            e422("The resulting config failed validation"),
                            e500("The config could not be saved"),
                        ],
                    ),
                ),
            ]),
        ));
    }
    for &(route, provider) in &crate::management::legacy::AUTH_URL_ROUTES {
        let id = camel(route);
        items.push((
            format!("/v0/management/{route}"),
            path_item(vec![(
                "get",
                mgmt_op(
                    true,
                    (
                        &format!("managementLegacyAuthUrl{id}"),
                        &format!("Start a {provider} login"),
                    ),
                    management_security(),
                    vec![
                        query_param("is_webui", "`1`/`true`/`yes`/`on` for the web UI flow"),
                        query_param("domain", "Kimi domain override"),
                        query_param("channel", "Kimi channel alias for `domain`"),
                    ],
                    None,
                    vec![ok("The login URL and its state.", "OAuthStart")],
                    vec![e404("The provider's login endpoint is unavailable")],
                ),
            )]),
        ));
    }
    items
}

/// `logs-max-total-size-mb` -> `LogsMaxTotalSizeMb`; used to build unique operationIds
/// from the legacy route names.
fn camel(slug: &str) -> String {
    let mut out = String::new();
    let mut upper = true;
    for c in slug.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(if upper { c.to_ascii_uppercase() } else { c });
            upper = false;
        } else {
            upper = true;
        }
    }
    out
}

fn schemas_list() -> Vec<(&'static str, Value)> {
    vec![
        (
            "ManagementError",
            json!({
                "type": "object",
                "description": "Error envelope of the management API: `{\"error\": \"...\"}` or \
            `{\"error\": \"...\", \"message\": \"...\"}`. Some handlers use a code in `error` (for example \
            `invalid_config`) and put the detail in `message`.",
                "properties": {
                    "error": { "type": "string" },
                    "message": { "type": "string" }
                },
                "required": ["error"],
                "additionalProperties": true
            }),
        ),
        (
            "ManagementConfig",
            json!({
                "type": "object",
                "description": "The management config document. Field names and shapes follow the \
            config file (v8 layout); TURN ICE `username`/`credential` are omitted from JSON reads.",
                "additionalProperties": true
            }),
        ),
        (
            "ConfigNode",
            json!({
                "type": ["object", "array", "string", "number", "boolean", "null"],
                "description": "The selected config value as JSON: an object, array or scalar \
            depending on the path.",
            }),
        ),
        (
            "ConfigWriteResult",
            json!({
                "type": "object",
                "properties": {
                    "config-version": { "type": "integer", "const": 8 },
                    "status": { "type": "string" }
                },
                "required": ["config-version", "status"],
                "additionalProperties": true
            }),
        ),
        (
            "ConfigYamlUploadResult",
            json!({
                "type": "object",
                "properties": {
                    "changed": { "type": "array", "items": { "type": "string" } },
                    "ok": { "type": "boolean" }
                },
                "required": ["changed", "ok"],
                "additionalProperties": true
            }),
        ),
        (
            "StatusOk",
            json!({
                "type": "object",
                "properties": { "status": { "type": "string" } },
                "required": ["status"],
                "additionalProperties": true
            }),
        ),
        (
            "CredentialList",
            json!({
                "type": "object",
                "properties": {
                    "observed_at": { "type": "string", "description": "RFC 3339 timestamp of the observation" },
                    "files": { "type": "array", "items": { "type": "object", "additionalProperties": true } },
                    "total": { "type": "integer" },
                    "page": { "type": "integer" },
                    "page_size": { "type": "integer" },
                    "has_more": { "type": "boolean" }
                },
                "required": ["observed_at", "files"],
                "additionalProperties": true
            }),
        ),
        (
            "CredentialMutation",
            json!({
                "type": "object",
                "description": "The result of an upload or delete. A partial batch answers 207 \
            with the `failed` entries.",
                "properties": {
                    "status": { "type": "string" },
                    "uploaded": { "type": "integer" },
                    "deleted": { "type": "integer" },
                    "files": { "type": "array", "items": { "type": "string" } },
                    "failed": { "type": "array", "items": { "type": "object", "additionalProperties": true } }
                },
                "required": ["status"],
                "additionalProperties": true
            }),
        ),
        (
            "CredentialStatus",
            json!({
                "type": "object",
                "properties": {
                    "status": { "type": "string" },
                    "disabled": { "type": "boolean" },
                    "via": { "type": "string" },
                    "excluded_pattern": { "type": "string" }
                },
                "required": ["status", "disabled"],
                "additionalProperties": true
            }),
        ),
        (
            "CredentialModels",
            json!({
                "type": "object",
                "properties": {
                    "models": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "string" },
                                "display_name": { "type": "string" },
                                "type": { "type": "string" },
                                "owned_by": { "type": "string" }
                            },
                            "required": ["id"],
                            "additionalProperties": true
                        }
                    }
                },
                "required": ["models"],
                "additionalProperties": true
            }),
        ),
        (
            "CredentialRefresh",
            json!({
                "type": "object",
                "properties": {
                    "ok": { "type": "boolean" },
                    "status": { "type": "string" },
                    "auth": { "type": "object", "additionalProperties": true },
                    "results": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "string" },
                                "success": { "type": "boolean" },
                                "error": { "type": "string" }
                            },
                            "additionalProperties": true
                        }
                    }
                },
                "additionalProperties": true
            }),
        ),
        (
            "ModelDefinitions",
            json!({
                "type": "object",
                "properties": {
                    "channel": { "type": "string" },
                    "models": { "type": "array", "items": { "type": "object", "additionalProperties": true } }
                },
                "required": ["channel", "models"],
                "additionalProperties": true
            }),
        ),
        (
            "ApiCallRequest",
            json!({
                "type": "object",
                "description": "A request to replay through the proxy. `$TOKEN$` in `header` \
            values or `data` is replaced with the selected credential's token.",
                "properties": {
                    "method": { "type": "string" },
                    "url": { "type": "string" },
                    "header": { "type": "object", "additionalProperties": { "type": "string" } },
                    "data": { "type": "string" },
                    "proxy_url": { "type": "string" },
                    "auth_index": { "type": "string" }
                },
                "required": ["method", "url"],
                "additionalProperties": true
            }),
        ),
        (
            "ApiCallResponse",
            json!({
                "type": "object",
                "properties": {
                    "status_code": { "type": "integer" },
                    "header": {
                        "type": "object",
                        "additionalProperties": { "type": "array", "items": { "type": "string" } }
                    },
                    "body": { "type": "string" }
                },
                "required": ["status_code", "header", "body"],
                "additionalProperties": true
            }),
        ),
        (
            "AuthIndexRequest",
            json!({
                "type": "object",
                "properties": { "auth_index": { "type": "string" } },
                "required": ["auth_index"],
                "additionalProperties": true
            }),
        ),
        (
            "CooldownReset",
            json!({
                "type": "object",
                "properties": {
                    "status": { "type": "string" },
                    "auth_index": { "type": "string" },
                    "models": { "type": "array", "items": { "type": "string" } },
                    "message": { "type": "string" }
                },
                "required": ["status", "auth_index", "models"],
                "additionalProperties": true
            }),
        ),
        (
            "ApiKeyUsage",
            json!({
                "type": "object",
                "description": "API-key credentials grouped by provider, then by `base_url|api_key`.",
                "additionalProperties": {
                    "type": "object",
                    "additionalProperties": {
                        "type": "object",
                        "properties": {
                            "success": { "type": "integer" },
                            "failed": { "type": "integer" },
                            "recent_requests": { "type": "array", "items": { "type": "object", "additionalProperties": true } }
                        },
                        "additionalProperties": true
                    }
                }
            }),
        ),
        (
            "UsageQueue",
            json!({
                "type": "array",
                "description": "Usage records in insertion order; a record that is valid JSON is \
            embedded as an object, anything else as a string.",
                "items": {}
            }),
        ),
        (
            "LogsPage",
            json!({
                "type": "object",
                "properties": {
                    "lines": { "type": "array", "items": { "type": "string" } },
                    "line-count": { "type": "integer" },
                    "latest-timestamp": { "type": "integer" },
                    "next-cursor": { "type": "string" },
                    "cursor-reset": { "type": "boolean" }
                },
                "required": ["lines", "line-count", "latest-timestamp", "next-cursor"],
                "additionalProperties": true
            }),
        ),
        (
            "ErrorLogs",
            json!({
                "type": "object",
                "properties": {
                    "files": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string" },
                                "size": { "type": "integer" },
                                "modified": { "type": "integer" }
                            },
                            "required": ["name", "size", "modified"],
                            "additionalProperties": true
                        }
                    }
                },
                "required": ["files"],
                "additionalProperties": true
            }),
        ),
        (
            "LatestVersion",
            json!({
                "type": "object",
                "properties": { "latest-version": { "type": "string" } },
                "required": ["latest-version"],
                "additionalProperties": true
            }),
        ),
        (
            "OAuthStart",
            json!({
                "type": "object",
                "properties": {
                    "status": { "type": "string" },
                    "url": { "type": "string" },
                    "state": { "type": "string" }
                },
                "required": ["status", "url", "state"],
                "additionalProperties": true
            }),
        ),
        (
            "OAuthStatus",
            json!({
                "type": "object",
                "description": "`{\"status\":\"ok\"}`, `{\"status\":\"wait\"}`, or \
            `{\"status\":\"error\",\"error\":\"...\"}`.",
                "properties": {
                    "status": { "type": "string" },
                    "error": { "type": "string" }
                },
                "required": ["status"],
                "additionalProperties": true
            }),
        ),
        (
            "OAuthCancel",
            json!({
                "type": "object",
                "properties": {
                    "status": { "type": "string" },
                    "cancelled": { "type": "boolean" }
                },
                "required": ["status", "cancelled"],
                "additionalProperties": true
            }),
        ),
        (
            "OAuthCallbackRequest",
            json!({
                "type": "object",
                "description": "The callback POST body. Only the first JSON value is read; \
            string fields, `null` ignored.",
                "properties": {
                    "provider": { "type": "string" },
                    "redirect_url": { "type": "string" },
                    "code": { "type": "string" },
                    "state": { "type": "string" },
                    "error": { "type": "string" }
                },
                "additionalProperties": true
            }),
        ),
        (
            "QuotaRequest",
            json!({
                "type": "object",
                "properties": {
                    "auth_index": { "type": "string" },
                    "plugin_id": { "type": "string" },
                    "provider": { "type": "string" }
                },
                "additionalProperties": true
            }),
        ),
        (
            "QuotaResponse",
            json!({
                "type": "object",
                "description": "A provider- or plugin-defined quota snapshot, returned as the \
            plugin's Go JSON.",
                "additionalProperties": true
            }),
        ),
        (
            "QuotaProviders",
            json!({
                "type": "object",
                "properties": {
                    "providers": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "plugin_id": { "type": "string" },
                                "provider": { "type": "string" },
                                "display_name": { "type": "string" },
                                "supported_providers": { "type": "array", "items": { "type": "string" } },
                                "supports_reset": { "type": "boolean" }
                            },
                            "required": ["plugin_id", "provider", "supports_reset"],
                            "additionalProperties": true
                        }
                    }
                },
                "required": ["providers"],
                "additionalProperties": true
            }),
        ),
        (
            "PluginsList",
            json!({
                "type": "object",
                "properties": {
                    "plugins_enabled": { "type": "boolean" },
                    "plugins_dir": { "type": "string" },
                    "plugins": { "type": "array", "items": { "type": "object", "additionalProperties": true } }
                },
                "required": ["plugins_enabled", "plugins_dir", "plugins"],
                "additionalProperties": true
            }),
        ),
        (
            "PluginStoreList",
            json!({
                "type": "object",
                "properties": {
                    "plugins_enabled": { "type": "boolean" },
                    "plugins_dir": { "type": "string" },
                    "sources": { "type": "array", "items": { "type": "object", "additionalProperties": true } },
                    "plugins": { "type": "array", "items": { "type": "object", "additionalProperties": true } }
                },
                "required": ["plugins_enabled", "plugins_dir", "plugins"],
                "additionalProperties": true
            }),
        ),
        (
            "PluginInstallRequest",
            json!({
                "type": "object",
                "properties": { "version": { "type": "string" } },
                "additionalProperties": true
            }),
        ),
        (
            "PluginInstall",
            json!({
                "type": "object",
                "properties": {
                    "status": { "type": "string" },
                    "source_id": { "type": "string" },
                    "source_name": { "type": "string" },
                    "source_url": { "type": "string" },
                    "id": { "type": "string" },
                    "version": { "type": "string" },
                    "install_type": { "type": "string" },
                    "path": { "type": "string" },
                    "plugins_enabled": { "type": "boolean" },
                    "restart_required": { "type": "boolean" }
                },
                "required": ["status", "id", "version"],
                "additionalProperties": true
            }),
        ),
        (
            "PluginConfig",
            json!({
                "type": "object",
                "description": "A plugin's config subtree; an unconfigured plugin answers `{}`.",
                "additionalProperties": true
            }),
        ),
        (
            "PluginEnabledRequest",
            json!({
                "type": "object",
                "properties": { "enabled": { "type": "boolean" } },
                "required": ["enabled"],
                "additionalProperties": true
            }),
        ),
        (
            "FieldValue",
            json!({
                "type": "object",
                "description": "A single config value under its field key (for example \
            `{\"debug\": true}`).",
                "additionalProperties": true
            }),
        ),
        (
            "FieldWriteBody",
            json!({
                "type": "object",
                "properties": { "value": { "type": ["boolean", "integer", "string"] } },
                "required": ["value"],
                "additionalProperties": true
            }),
        ),
        (
            "ListValue",
            json!({
                "type": "object",
                "description": "A provider key list or OAuth map under its route key (for example \
            `{\"gemini-api-key\": [...]}`).",
                "additionalProperties": true
            }),
        ),
        (
            "ListWriteBody",
            json!({
                "type": ["array", "object"],
                "description": "The provider key list or OAuth map: a JSON array, or an object with \
            an `items` member holding it (Go's flexible decode).",
                "items": { "additionalProperties": true },
                "additionalProperties": true
            }),
        ),
        (
            "ListPatchBody",
            json!({
                "type": "object",
                "description": "A list edit: `{old, new}` or `{index, value}` (Go's \
            `patchStringList`), or the list itself.",
                "additionalProperties": true
            }),
        ),
        (
            "PluginDelete",
            json!({
                "type": "object",
                "properties": {
                    "status": { "type": "string", "const": "deleted" },
                    "id": { "type": "string" },
                    "path": { "type": "string" },
                    "file_deleted": { "type": "boolean" },
                    "configured_removed": { "type": "boolean" },
                    "restart_required": { "type": "boolean" }
                },
                "required": ["status", "id"],
                "additionalProperties": true
            }),
        ),
        (
            "LogsClearResult",
            json!({
                "type": "object",
                "properties": {
                    "success": { "type": "boolean" },
                    "message": { "type": "string" },
                    "removed": { "type": "integer" }
                },
                "required": ["success", "message", "removed"],
                "additionalProperties": true
            }),
        ),
        (
            "ApiKeys",
            json!({
                "type": "object",
                "properties": { "api-keys": { "type": "array", "items": { "type": "string" } } },
                "required": ["api-keys"],
                "additionalProperties": true
            }),
        ),
        (
            "CredentialFile",
            json!({
                "type": "object",
                "description": "A credential auth file as JSON.",
                "additionalProperties": true
            }),
        ),
        (
            "CredentialDeleteRequest",
            json!({
                "type": ["array", "object"],
                "description": "The names to delete: a JSON array of strings, or `{\"name\", \"names\"}`.",
                "items": { "type": "string" },
                "properties": {
                    "name": { "type": "string" },
                    "names": { "type": "array", "items": { "type": "string" } }
                },
                "additionalProperties": true
            }),
        ),
        (
            "CredentialStatusRequest",
            json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "auth_index": { "type": "string" },
                    "disabled": { "type": "boolean" }
                },
                "required": ["name", "disabled"],
                "additionalProperties": true
            }),
        ),
        (
            "CredentialFieldsRequest",
            json!({
                "type": "object",
                "description": "The credential's `name` plus the metadata fields to patch, keyed by dotted path (for example `headers.X-Api-Key`).",
                "properties": { "name": { "type": "string" } },
                "required": ["name"],
                "additionalProperties": true
            }),
        ),
        (
            "CredentialRefreshRequest",
            json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "auth_index": { "type": "string" },
                    "all": { "type": "boolean" }
                },
                "additionalProperties": true
            }),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    const METHODS: [&str; 8] = ["get", "put", "post", "delete", "options", "head", "patch", "trace"];

    fn base() -> Value {
        json!({
            "openapi": "3.1.0",
            "info": { "title": "CLIProxyAPI", "version": "0" },
            "servers": [{ "url": "/" }],
            "tags": [{ "name": "General", "description": "general" }],
            "paths": { "/healthz": { "get": { "operationId": "healthz", "responses": {} } } },
            "components": {
                "securitySchemes": { "clientBearer": { "type": "http", "scheme": "bearer" } },
                "schemas": { "JsonBody": { "type": "object" } }
            }
        })
    }

    fn merged() -> Value {
        let mut document = base();
        extend(&mut document);
        document
    }

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

    fn scheme_names(security: &Value) -> Vec<String> {
        security
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|alternative| alternative.as_object().and_then(|m| m.keys().next().cloned()))
            .collect()
    }

    /// The complete fixed v0/v8 path-method inventory. The legacy route names come from
    /// the same product constants the router builds from, so this cannot drift from them.
    fn inventory() -> Vec<(String, &'static str)> {
        let mut v: Vec<(String, &'static str)> = Vec::new();
        let mut add = |path: &str, method: &'static str| v.push((path.to_owned(), method));
        for (path, methods) in [
            ("/v8/management/config", &["get", "put", "patch"][..]),
            ("/v8/management/config.yaml", &["get", "put"][..]),
            ("/v8/management/config/", &["get", "put", "patch", "delete"][..]),
            ("/v8/management/config/{path}", &["get", "put", "patch", "delete"][..]),
            ("/v8/management/server/latest-version", &["get"][..]),
            ("/v8/management/requests/api-call", &["post"][..]),
            ("/v8/management/routing/cooldown/reset", &["post"][..]),
            ("/v8/management/observability/usage/api-keys", &["get"][..]),
            ("/v8/management/observability/usage/queue", &["get"][..]),
            ("/v8/management/observability/logs", &["get", "delete"][..]),
            ("/v8/management/observability/logs/errors", &["get"][..]),
            ("/v8/management/observability/logs/errors/{name}", &["get"][..]),
            ("/v8/management/observability/logs/requests/{id}", &["get"][..]),
            ("/v8/management/oauth/auth-url", &["get"][..]),
            ("/v8/management/oauth/status", &["get"][..]),
            ("/v8/management/oauth/session", &["delete"][..]),
            ("/v8/management/oauth/import", &["post"][..]),
            ("/v8/management/oauth/callback", &["get", "post"][..]),
            ("/v8/management/quota/fetch", &["post"][..]),
            ("/v8/management/plugins", &["get"][..]),
            ("/v8/management/plugins/{id}", &["delete"][..]),
            ("/v8/management/plugins/store", &["get", "delete"][..]),
            ("/v8/management/plugins/store/{id}/install", &["post"][..]),
            ("/v8/management/plugins/{id}/quota", &["get", "post", "delete"][..]),
            ("/v0/management/config.yaml", &["get", "put"][..]),
            ("/v0/management/config", &["get"][..]),
            ("/v0/management/latest-version", &["get"][..]),
            ("/v0/management/api-call", &["post"][..]),
            ("/v0/management/reset-quota", &["post"][..]),
            ("/v0/management/api-key-usage", &["get"][..]),
            ("/v0/management/usage-queue", &["get"][..]),
            ("/v0/management/get-auth-status", &["get"][..]),
            ("/v0/management/oauth-session", &["delete"][..]),
            ("/v0/management/oauth-callback", &["get", "post"][..]),
            ("/v0/management/quota/providers", &["get"][..]),
            ("/v0/management/quota/fetch", &["post"][..]),
            ("/v0/management/quota/reset", &["post"][..]),
            ("/v0/management/vertex/import", &["post"][..]),
            ("/v0/management/logs", &["get", "delete"][..]),
            ("/v0/management/request-error-logs", &["get"][..]),
            ("/v0/management/request-error-logs/{name}", &["get"][..]),
            ("/v0/management/request-log-by-id/{id}", &["get"][..]),
            ("/v0/management/plugins", &["get"][..]),
            ("/v0/management/plugins/{id}", &["delete"][..]),
            ("/v0/management/plugins/{id}/quota", &["get", "post", "delete"][..]),
            ("/v0/management/plugins/{id}/quota/reset", &["post"][..]),
            ("/v0/management/plugins/{id}/enabled", &["patch"][..]),
            ("/v0/management/plugins/{id}/config", &["get", "put", "patch"][..]),
            ("/v0/management/plugin-store", &["get"][..]),
            ("/v0/management/plugin-store/{id}/install", &["post"][..]),
            ("/v0/management/api-keys", &["get", "put", "patch", "delete"][..]),
        ] {
            for method in methods {
                add(path, method);
            }
        }
        for (base, files, definitions) in [
            ("/v8/management", "credentials", "routing/model-definitions"),
            ("/v0/management", "auth-files", "model-definitions"),
        ] {
            for (suffix, methods) in [
                ("", &["get", "post", "delete"][..]),
                ("/models", &["get"][..]),
                ("/download", &["get"][..]),
                ("/status", &["patch"][..]),
                ("/fields", &["patch"][..]),
                ("/refresh", &["post"][..]),
            ] {
                for method in methods {
                    add(&format!("{base}/{files}{suffix}"), method);
                }
            }
            add(&format!("{base}/{definitions}/{{channel}}"), "get");
        }
        for &route in &crate::management::legacy::FIELD_ROUTES {
            for method in ["get", "put", "patch"] {
                add(&format!("/v0/management/{route}"), method);
            }
        }
        add("/v0/management/proxy-url", "delete");
        for &route in &crate::management::legacy::LIST_ROUTES {
            for method in ["get", "put", "patch", "delete"] {
                add(&format!("/v0/management/{route}"), method);
            }
        }
        for &(route, _) in &crate::management::legacy::AUTH_URL_ROUTES {
            add(&format!("/v0/management/{route}"), "get");
        }
        v
    }

    #[test]
    fn merges_without_touching_general_entries() {
        let document = merged();
        assert_eq!(document["paths"]["/healthz"]["get"]["operationId"], "healthz");
        assert_eq!(
            document["components"]["securitySchemes"]["clientBearer"]["type"],
            "http"
        );
        assert_eq!(document["components"]["schemas"]["JsonBody"]["type"], "object");
        let tag_names: Vec<&str> = document["tags"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["name"].as_str())
            .collect();
        assert!(tag_names.contains(&"General"), "the general tag must survive");
        assert!(tag_names.contains(&TAG_MANAGEMENT));
        assert!(tag_names.contains(&TAG_LEGACY));
        assert!(
            document["paths"]["/v8/management/config"].is_object(),
            "management paths must be added"
        );
    }

    #[test]
    fn every_fixed_management_route_is_present() {
        let document = merged();
        let mut missing = Vec::new();
        for (path, method) in inventory() {
            if document["paths"][&path][method].as_object().is_none() {
                missing.push(format!("{method} {path}"));
            }
        }
        assert!(missing.is_empty(), "missing management routes: {missing:?}");
    }

    #[test]
    fn no_general_path_is_shadowed_and_no_foreign_path_is_added() {
        let document = merged();
        for (path, _, _) in operations(&document) {
            assert!(
                path.starts_with("/v8/management") || path.starts_with("/v0/management") || path == "/healthz",
                "unexpected path in the merged document: {path}"
            );
        }
    }

    #[test]
    fn v0_operations_are_deprecated_and_v8_are_not() {
        let document = merged();
        for (path, method, operation) in operations(&document) {
            let deprecated = operation["deprecated"].as_bool().unwrap_or(false);
            if path.starts_with("/v0/management") {
                assert!(deprecated, "{method} {path} must be deprecated");
                assert_eq!(operation["tags"][0], TAG_LEGACY);
            } else if path.starts_with("/v8/management") {
                assert!(!deprecated, "{method} {path} must not be deprecated");
                assert_eq!(operation["tags"][0], TAG_MANAGEMENT);
            }
        }
    }

    #[test]
    fn management_key_schemes_are_distinct_and_callbacks_are_public() {
        let document = merged();
        let schemes = &document["components"]["securitySchemes"];
        assert_eq!(schemes["managementBearer"]["type"], "http");
        assert_eq!(schemes["managementBearer"]["scheme"], "bearer");
        assert_eq!(schemes["managementKey"]["type"], "apiKey");
        assert_eq!(schemes["managementKey"]["in"], "header");
        assert_eq!(schemes["managementKey"]["name"], "X-Management-Key");
        assert_eq!(
            schemes["clientBearer"]["type"], "http",
            "the client scheme must survive"
        );

        let names = scheme_names(&document["paths"]["/v8/management/config"]["get"]["security"]);
        assert!(names.contains(&"managementBearer".to_owned()), "{names:?}");
        assert!(names.contains(&"managementKey".to_owned()), "{names:?}");
        assert!(
            !names.contains(&"clientBearer".to_owned()),
            "management routes must not offer the client scheme"
        );

        for (path, method) in [
            ("/v8/management/oauth/callback", "get"),
            ("/v8/management/oauth/callback", "post"),
            ("/v0/management/oauth-callback", "get"),
            ("/v0/management/oauth-callback", "post"),
        ] {
            assert_eq!(
                document["paths"][path][method]["security"],
                json!([]),
                "{method} {path} must take no key"
            );
        }
    }

    #[test]
    fn yaml_and_binary_endpoints_are_not_mislabeled_json() {
        let document = merged();
        let ok_content =
            |path: &str, method: &str| document["paths"][path][method]["responses"]["200"]["content"].clone();
        let request_content =
            |path: &str, method: &str| document["paths"][path][method]["requestBody"]["content"].clone();

        let yaml = ok_content("/v8/management/config.yaml", "get");
        assert!(yaml.get(YAML).is_some(), "config.yaml GET must be YAML");
        assert!(yaml.get(JSON).is_none(), "config.yaml GET must not be JSON");

        let upload = request_content("/v0/management/config.yaml", "put");
        assert!(upload.get(YAML).is_some(), "config.yaml PUT must accept YAML");

        let download = ok_content("/v8/management/credentials/download", "get");
        assert!(download.get(JSON).is_some(), "credential download is a JSON file");

        for (path, method) in [
            ("/v8/management/observability/logs/errors/{name}", "get"),
            ("/v8/management/observability/logs/requests/{id}", "get"),
            ("/v0/management/request-error-logs/{name}", "get"),
            ("/v0/management/request-log-by-id/{id}", "get"),
        ] {
            assert!(
                ok_content(path, method).get(OCTET).is_some(),
                "{method} {path} must be a binary download"
            );
        }

        let upload = request_content("/v8/management/credentials", "post");
        assert!(
            upload.get(MULTIPART).is_some(),
            "credential upload must accept multipart"
        );
        assert!(upload.get(JSON).is_some(), "credential upload must accept JSON");
    }

    #[test]
    fn operation_ids_are_unique_present_and_well_formed() {
        let document = merged();
        let ids = operation_ids(&document);
        assert!(!ids.is_empty());
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for id in &ids {
            *counts.entry(id.clone()).or_insert(0) += 1;
        }
        let duplicates: Vec<String> = counts
            .iter()
            .filter(|(_, n)| **n > 1)
            .map(|(id, _)| id.clone())
            .collect();
        assert!(duplicates.is_empty(), "duplicate operationIds: {duplicates:?}");
        for id in &ids {
            assert!(
                id.chars().next().is_some_and(|c| c.is_ascii_alphabetic()),
                "operationId must start with a letter: {id}"
            );
        }
    }

    #[test]
    fn local_refs_resolve() {
        let document = merged();
        let dangling = dangling_refs(&document);
        assert!(dangling.is_empty(), "dangling $refs: {dangling:?}");
    }

    #[test]
    fn every_path_placeholder_is_a_required_parameter() {
        let document = merged();
        let mut missing = Vec::new();
        for (path, method, operation) in operations(&document) {
            let declared: Vec<&str> = operation["parameters"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|p| p["in"] == "path" && p["required"] == true)
                .filter_map(|p| p["name"].as_str())
                .collect();
            for name in placeholders(&path) {
                if !declared.contains(&name.as_str()) {
                    missing.push(format!("{method} {path}: `{name}`"));
                }
            }
        }
        assert!(missing.is_empty(), "path parameters not declared required: {missing:?}");
    }

    /// Every component schema carries a `type` or `$ref`, so the merged document keeps
    /// the invariant the general document's own test asserts.
    #[test]
    fn component_schemas_are_well_formed() {
        let document = merged();
        let mut malformed = Vec::new();
        if let Some(schemas) = document["components"]["schemas"].as_object() {
            for (name, schema) in schemas {
                if !(schema.is_object() && (schema.get("type").is_some() || schema.get("$ref").is_some())) {
                    malformed.push(name.clone());
                }
            }
        }
        assert!(malformed.is_empty(), "malformed schemas: {malformed:?}");
    }

    #[test]
    fn camel_makes_unique_id_fragments() {
        assert_eq!(camel("logs-max-total-size-mb"), "LogsMaxTotalSizeMb");
        assert_eq!(camel("quota-exceeded/switch-project"), "QuotaExceededSwitchProject");
        assert_eq!(camel("routing/strategy"), "RoutingStrategy");
    }

    #[test]
    fn extend_is_idempotent_on_paths() {
        let mut document = base();
        extend(&mut document);
        let first = operation_ids(&document).len();
        extend(&mut document);
        assert_eq!(operation_ids(&document).len(), first, "re-extending must not duplicate");
    }
}
