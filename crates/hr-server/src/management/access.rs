//! Management access control, ported from Go's `Handler.Middleware` and
//! `AuthenticateManagementKey` (internal/api/handlers/management/handler.go), the
//! availability gate (internal/api/server_management.go, server_reload.go) and the
//! global CORS middleware (internal/api/server_middleware.go).
//!
//! Order is Go's, with one intentional difference (docs/DIFFERENCES-FROM-GO.md):
//! management is always available and, when no password is set anywhere (the
//! ADMIN_PASSWORD environment variable, the config `management.secret-key`, or a
//! `--password`), every request is accepted without a key. Go instead turns management
//! off (404) without a secret. Otherwise: version headers -> active ban (403) -> remote
//! policy (403) -> missing key (401, counted) -> local password -> ADMIN_PASSWORD ->
//! bcrypt (401, counted). The fifth counted failure bans the client IP for 30 minutes.
//! "Local" means the client IP text is exactly `127.0.0.1` or `::1`, after
//! trusted-proxy resolution.
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use hr_core::config::{Config, TrustedProxies, go_trim_space};
use subtle::ConstantTimeEq;

use super::{Management, json_error};

const MAX_FAILURES: u32 = 5;
const BAN: Duration = Duration::from_secs(30 * 60);
/// Go `attemptCleanupInterval`.
pub(super) const PURGE_EVERY: Duration = Duration::from_secs(3600);
const MAX_IDLE: Duration = Duration::from_secs(2 * 3600);

pub const VERSION: &str = concat!("hyper-router-", env!("CARGO_PKG_VERSION"));
const COMMIT: &str = match option_env!("CPA_COMMIT") {
    Some(v) => v,
    None => "none",
};
const BUILD_DATE: &str = match option_env!("CPA_BUILD_DATE") {
    Some(v) => v,
    None => "unknown",
};
/// Go reports "1" only for cgo builds that can load native plugins.
const SUPPORT_PLUGIN: &str = hr_plugin::SUPPORT_PLUGIN;

const EXPOSED_HEADERS: &str = "X-CPA-TRACE-ID, X-CPA-VERSION, X-CPA-COMMIT, X-CPA-BUILD-DATE, \
X-CPA-SUPPORT-PLUGIN, X-CPA-HOME-VERSION, X-CPA-HOME-BUILD-DATE, X-SERVER-VERSION, \
X-SERVER-BUILD-DATE, Location, Retry-After, X-Request-Id, OpenAI-Request-Id";

struct Attempt {
    count: u32,
    blocked_until: Option<Instant>,
    last_activity: Instant,
}

struct Attempts {
    by_ip: HashMap<String, Attempt>,
}

pub(crate) struct Access {
    /// Raw bytes, as Go keeps arbitrary environment strings.
    env_secret: Vec<u8>,
    local_password: String,
    /// Startup-only, as gin's `SetTrustedProxies` is called once in `NewServer`.
    trusted: TrustedProxies,
    attempts: Mutex<Attempts>,
    warned_untrusted_forwarding: AtomicBool,
}

/// An environment value as Go reads it: raw bytes on Unix; on Windows the UTF-16
/// value decoded to UTF-8 with invalid units replaced (Go `syscall.Getenv`).
fn env_bytes(key: &str) -> Vec<u8> {
    let Some(value) = std::env::var_os(key) else {
        return Vec::new();
    };
    #[cfg(unix)]
    return std::os::unix::ffi::OsStringExt::into_vec(value);
    #[cfg(not(unix))]
    return value.to_string_lossy().into_owned().into_bytes();
}

/// Go `startAttemptCleanup`: an hourly purge for as long as the management state
/// lives. Without a Tokio runtime (synchronous callers) there is no timer.
pub(super) fn start_purge(state: &std::sync::Arc<Management>) {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        return;
    };
    let state = std::sync::Arc::downgrade(state);
    runtime.spawn(async move {
        let mut ticks = tokio::time::interval_at(tokio::time::Instant::now() + PURGE_EVERY, PURGE_EVERY);
        loop {
            ticks.tick().await;
            let Some(state) = state.upgrade() else { return };
            state.access.purge_stale(Instant::now());
        }
    });
}

impl Access {
    pub(crate) fn new(cfg: &Config, options: super::Options) -> Self {
        // Go: os.LookupEnv + TrimSpace; empty means unset.
        let raw: Vec<u8> = match options.admin_password {
            Some(value) => value.into_bytes(),
            None => env_bytes("ADMIN_PASSWORD"),
        };
        let env_secret = go_trim_space(&raw).to_vec();
        let local_password = options.local_password;
        Self {
            env_secret,
            local_password,
            trusted: TrustedProxies::new(&cfg.trusted_proxies),
            attempts: Mutex::new(Attempts { by_ip: HashMap::new() }),
            warned_untrusted_forwarding: AtomicBool::new(false),
        }
    }

    /// Management is always available, whatever the password. Go turns it off (404) when
    /// no secret is configured; hyper-router leaves it open and accepts any key
    /// (docs/DIFFERENCES-FROM-GO.md).
    pub(crate) fn available(&self) -> bool {
        true
    }

    pub(crate) fn client_ip(&self, peer: Option<SocketAddr>, headers: &HeaderMap) -> String {
        let ip = self
            .trusted
            .client_ip(peer, |name| headers.get(name).map(HeaderValue::as_bytes));
        // Go trusts no proxy by default, so a local reverse proxy or tunnel (for example
        // cloudflared on loopback) makes every client local. Keep Go's decision but say
        // so once; the fix is `server.trusted-proxies: [127.0.0.1, "::1"]` and a restart.
        if (ip == "127.0.0.1" || ip == "::1")
            && ["X-Forwarded-For", "X-Real-IP", "CF-Connecting-IP"]
                .iter()
                .any(|h| headers.contains_key(*h))
            && !self.warned_untrusted_forwarding.swap(true, Ordering::Relaxed)
        {
            tracing::warn!(
                "management request from a loopback proxy carries forwarding headers; it is treated as \
                 local. Set server.trusted-proxies to the proxy address and restart to use the client address"
            );
        }
        ip
    }

    fn attempts(&self) -> std::sync::MutexGuard<'_, Attempts> {
        self.attempts.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Go `purgeStaleAttempts`: drops records idle longer than two hours unless still
    /// banned. Runs from the hourly timer [`start_purge`] installs.
    pub(super) fn purge_stale(&self, now: Instant) {
        self.attempts().by_ip.retain(|_, a| {
            a.blocked_until.is_some_and(|until| now < until) || now.duration_since(a.last_activity) <= MAX_IDLE
        });
    }

    fn fail(&self, ip: &str) {
        let now = Instant::now();
        let mut attempts = self.attempts();
        let entry = attempts.by_ip.entry(ip.to_owned()).or_insert(Attempt {
            count: 0,
            blocked_until: None,
            last_activity: now,
        });
        entry.count += 1;
        entry.last_activity = now;
        if entry.count >= MAX_FAILURES {
            entry.blocked_until = Some(now + BAN);
            entry.count = 0;
        }
    }

    fn reset(&self, ip: &str) {
        if let Some(entry) = self.attempts().by_ip.get_mut(ip) {
            entry.count = 0;
            entry.blocked_until = None;
        }
    }

    /// Go `AuthenticateManagementKey`. The error is the status and `error` text.
    pub(crate) async fn authenticate(
        &self,
        cfg: &Config,
        ip: &str,
        local: bool,
        provided: &[u8],
    ) -> Result<(), (StatusCode, String)> {
        {
            let now = Instant::now();
            let mut attempts = self.attempts();
            if let Some(entry) = attempts.by_ip.get_mut(ip)
                && let Some(until) = entry.blocked_until
            {
                if now < until {
                    let remaining = go_duration_seconds(until - now);
                    return Err((
                        StatusCode::FORBIDDEN,
                        format!("IP banned due to too many failed attempts. Try again in {remaining}"),
                    ));
                }
                entry.blocked_until = None;
                entry.count = 0;
            }
        }
        let allow_remote = cfg.management.allow_remote || !self.env_secret.is_empty();
        if !local && !allow_remote {
            return Err((StatusCode::FORBIDDEN, "remote management disabled".into()));
        }
        let secret = cfg.management.secret_key.clone();
        // No password anywhere: management is open, and every request is accepted whether
        // or not it carries a key. Go refuses with "remote management key not set" instead
        // (docs/DIFFERENCES-FROM-GO.md).
        if secret.is_empty() && self.env_secret.is_empty() && self.local_password.is_empty() {
            self.reset(ip);
            return Ok(());
        }
        if provided.is_empty() {
            self.fail(ip);
            return Err((StatusCode::UNAUTHORIZED, "missing management key".into()));
        }
        let matches = |want: &[u8]| !want.is_empty() && bool::from(want.ct_eq(provided));
        if (local && matches(self.local_password.as_bytes())) || matches(&self.env_secret) {
            self.reset(ip);
            return Ok(());
        }
        let candidate = provided.to_vec();
        let valid = !secret.is_empty()
            && tokio::task::spawn_blocking(move || bcrypt::verify(candidate, &secret).unwrap_or(false))
                .await
                .unwrap_or(false);
        if !valid {
            self.fail(ip);
            return Err((StatusCode::UNAUTHORIZED, "invalid management key".into()));
        }
        self.reset(ip);
        Ok(())
    }
}

/// `Duration.Round(time.Second).String()` for non-negative whole seconds.
fn go_duration_seconds(d: Duration) -> String {
    let secs = (d.as_nanos() + 500_000_000) / 1_000_000_000;
    let (h, m, s) = (secs / 3600, secs % 3600 / 60, secs % 60);
    if h > 0 {
        format!("{h}h{m}m{s}s")
    } else if m > 0 {
        format!("{m}m{s}s")
    } else {
        format!("{s}s")
    }
}

/// `Authorization: Bearer <key>` (scheme case-insensitive, value untrimmed), any
/// other Authorization value verbatim, then `X-Management-Key`. First values only.
fn provided_key(headers: &HeaderMap) -> &[u8] {
    let auth = headers
        .get(header::AUTHORIZATION)
        .map(HeaderValue::as_bytes)
        .unwrap_or_default();
    let key = match auth.iter().position(|&b| b == b' ') {
        Some(i) if auth[..i].eq_ignore_ascii_case(b"bearer") => &auth[i + 1..],
        _ => auth,
    };
    if key.is_empty() {
        headers
            .get("X-Management-Key")
            .map(HeaderValue::as_bytes)
            .unwrap_or_default()
    } else {
        key
    }
}

fn version_headers(headers: &mut HeaderMap) {
    for (name, value) in [
        ("X-CPA-VERSION", VERSION),
        ("X-CPA-COMMIT", COMMIT),
        ("X-CPA-BUILD-DATE", BUILD_DATE),
        ("X-CPA-SUPPORT-PLUGIN", SUPPORT_PLUGIN),
    ] {
        headers.insert(name, HeaderValue::from_static(value));
    }
}

pub(crate) fn not_found() -> Response {
    StatusCode::NOT_FOUND.into_response()
}

/// Go `managementAvailabilityMiddleware` alone (the OAuth callback routes).
pub(crate) async fn available(State(state): State<Arc<Management>>, req: Request, next: Next) -> Response {
    if !state.access.available() {
        return not_found();
    }
    next.run(req).await
}

/// Availability plus `Handler.Middleware`, applied per matched route and method.
pub(crate) async fn guard(State(state): State<Arc<Management>>, req: Request, next: Next) -> Response {
    if !state.access.available() {
        return not_found();
    }
    let peer = req.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0);
    let ip = state.access.client_ip(peer, req.headers());
    let local = ip == "127.0.0.1" || ip == "::1";
    let provided = provided_key(req.headers()).to_vec();
    let cfg = state.rt.config();
    let mut response = match state.access.authenticate(&cfg, &ip, local, &provided).await {
        Ok(()) => next.run(req).await,
        Err((status, message)) => json_error(status, &message),
    };
    version_headers(response.headers_mut());
    response
}

/// Go's global CORS middleware: every response is cross-origin readable and every
/// OPTIONS request ends with 204 before routing or authentication.
#[derive(Clone)]
pub(crate) struct Cors;

pub async fn cors(mut req: Request, next: Next) -> Response {
    req.extensions_mut().insert(Cors);
    let mut response = if req.method() == Method::OPTIONS {
        StatusCode::NO_CONTENT.into_response()
    } else {
        next.run(req).await
    };
    if response.status() == StatusCode::NOT_FOUND {
        // axum adds `Allow` to method fallbacks; gin's NoRoute 404 has none.
        response.headers_mut().remove(header::ALLOW);
    }
    cors_headers(&mut response);
    response
}

/// The inner request-log snapshot must see CORS headers before axum framing.
pub(crate) fn cors_headers(response: &mut Response) {
    let headers = response.headers_mut();
    for (name, value) in [
        ("Access-Control-Allow-Origin", "*"),
        ("Access-Control-Allow-Methods", "GET, POST, PUT, PATCH, DELETE, OPTIONS"),
        ("Access-Control-Allow-Headers", "*"),
        ("Access-Control-Expose-Headers", EXPOSED_HEADERS),
    ] {
        headers.insert(name, HeaderValue::from_static(value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_only_expire_through_the_hourly_purge() {
        let cfg = Config::parse(
            "management:\n  secret-key: '$2a$04$aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'\n",
        )
        .unwrap();
        let access = Access::new(&cfg, super::super::Options::default());
        for _ in 0..4 {
            access.fail("203.0.113.9");
        }
        access.fail("203.0.113.10");
        for _ in 0..5 {
            access.fail("198.51.100.1");
        }
        let count = |ip: &str| access.attempts().by_ip.get(ip).map(|a| a.count);
        // A purge within two hours of the last failure keeps every record.
        access.purge_stale(Instant::now() + MAX_IDLE - Duration::from_secs(60));
        assert_eq!(count("203.0.113.9"), Some(4));
        // Past the idle limit, idle records go; a ban still running stays.
        access.attempts().by_ip.get_mut("198.51.100.1").unwrap().blocked_until =
            Some(Instant::now() + MAX_IDLE + Duration::from_secs(600));
        access.purge_stale(Instant::now() + MAX_IDLE + Duration::from_secs(1));
        assert_eq!(count("203.0.113.9"), None);
        assert_eq!(count("203.0.113.10"), None);
        assert!(
            access.attempts().by_ip.contains_key("198.51.100.1"),
            "banned until +30m"
        );
        // The forgiven client starts a fresh count: no ban on its next failure.
        access.fail("203.0.113.9");
        assert_eq!(count("203.0.113.9"), Some(1));
        assert!(access.attempts().by_ip["203.0.113.9"].blocked_until.is_none());
    }

    /// With no password configured anywhere, management is open: any request is accepted,
    /// key or not, and no failure is counted. Go refuses with "remote management key not
    /// set" instead (docs/DIFFERENCES-FROM-GO.md). The remote policy still applies.
    #[tokio::test]
    async fn no_password_anywhere_opens_management() {
        let cfg = Config::parse("management:\n  allow-remote: false\n").unwrap();
        let access = Access::new(
            &cfg,
            super::super::Options {
                admin_password: Some(String::new()),
                ..Default::default()
            },
        );
        assert!(access.available());
        assert!(access.authenticate(&cfg, "127.0.0.1", true, b"").await.is_ok());
        assert!(access.authenticate(&cfg, "127.0.0.1", true, b"anything").await.is_ok());
        // A remote client is still refused unless allow-remote is on.
        let remote = access.authenticate(&cfg, "203.0.113.9", false, b"").await;
        assert_eq!(remote.unwrap_err().1, "remote management disabled");
        let open = Config::parse("management:\n  allow-remote: true\n").unwrap();
        let access = Access::new(
            &open,
            super::super::Options {
                admin_password: Some(String::new()),
                ..Default::default()
            },
        );
        assert!(access.authenticate(&open, "203.0.113.9", false, b"").await.is_ok());
    }

    /// A `--password` (or the standalone TUI's generated one) is the only password: it is
    /// required from loopback and refused from other hosts, with no configured secret.
    #[tokio::test]
    async fn local_password_is_required_when_it_is_the_only_password() {
        let cfg = Config::parse("management:\n  allow-remote: true\n").unwrap();
        let access = Access::new(
            &cfg,
            super::super::Options {
                local_password: "fake-local".into(),
                admin_password: Some(String::new()),
                ..Default::default()
            },
        );
        assert!(access.available());
        assert!(
            access
                .authenticate(&cfg, "127.0.0.1", true, b"fake-local")
                .await
                .is_ok()
        );
        let missing = access.authenticate(&cfg, "127.0.0.1", true, b"").await;
        assert_eq!(missing.unwrap_err().1, "missing management key");
        let wrong = access.authenticate(&cfg, "127.0.0.1", true, b"wrong").await;
        assert_eq!(wrong.unwrap_err().1, "invalid management key");
        let remote = access.authenticate(&cfg, "203.0.113.9", false, b"fake-local").await;
        assert_eq!(remote.unwrap_err().1, "invalid management key");
    }

    #[test]
    fn go_duration_rounds_half_up_to_seconds() {
        let cases = [
            (Duration::from_millis(1_799_400), "29m59s"),
            (Duration::from_millis(1_799_500), "30m0s"),
            (Duration::from_secs(1800), "30m0s"),
            (Duration::from_millis(400), "0s"),
            (Duration::from_secs(59), "59s"),
            (Duration::from_secs(3600), "1h0m0s"),
            (Duration::from_secs(3661), "1h1m1s"),
        ];
        for (d, want) in cases {
            assert_eq!(go_duration_seconds(d), want, "{d:?}");
        }
    }

    #[test]
    fn key_extraction_matches_go_header_rules() {
        let get = |pairs: &[(&'static str, &str)]| {
            let mut h = HeaderMap::new();
            for (k, v) in pairs {
                h.append(*k, v.parse().unwrap());
            }
            String::from_utf8(provided_key(&h).to_vec()).unwrap()
        };
        assert_eq!(get(&[("authorization", "Bearer  k")]), " k");
        assert_eq!(get(&[("authorization", "bEaReR k")]), "k");
        assert_eq!(
            get(&[("authorization", "Basic k"), ("x-management-key", "m")]),
            "Basic k"
        );
        assert_eq!(get(&[("authorization", "Bearer "), ("x-management-key", "m")]), "m");
        assert_eq!(
            get(&[("authorization", "Bearer a"), ("authorization", "Bearer b")]),
            "a"
        );
        assert_eq!(get(&[("authorization", "raw")]), "raw");
    }
}
