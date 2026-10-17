//! Automatic Tailscale HTTPS exposure through `tailscale serve`, horon-router only.
//!
//! When this machine is connected to a tailnet, the listener is published as
//! `https://<machine>.<tailnet>.ts.net[:port]` by `tailscale serve`, which terminates TLS
//! with the node's own certificate. No certificate files, no second listener and no
//! configuration write: the mapping lives in tailscaled and is re-applied at every start.
//!
//! Skipped when the standalone TUI owns the listener, when `server.tls` is on (`serve`
//! cannot trust a self-signed backend) and when the listener port is ephemeral.
//! `HORON_TAILSCALE_HTTPS=off` turns it off, `HORON_TAILSCALE_HTTPS_PORT` pins the
//! exposed port, and `HORON_TAILSCALE_BIN` names the CLI.
//!
//! A port that already serves another local app is never taken over unless the port was
//! pinned by hand: the first free port of [`CANDIDATES`] is used instead, so restarting
//! horon-router does not disturb the mappings of other programs. Go has no counterpart;
//! see docs/DIFFERENCES-FROM-GO.md.

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

use horon_core::config::Config;

/// A `tailscale` call that outlives this is a stuck daemon, not a slow answer.
const TIMEOUT: Duration = Duration::from_secs(10);
const ENV_ENABLE: &str = "HORON_TAILSCALE_HTTPS";
const ENV_PORT: &str = "HORON_TAILSCALE_HTTPS_PORT";
const ENV_BIN: &str = "HORON_TAILSCALE_BIN";
/// Ports tried in order when earlier ones already serve something else. 443 is what a
/// browser reaches without a suffix; the rest are the conventional alternates.
const CANDIDATES: &[u16] = &[443, 8443, 10000];
/// When every candidate is taken, the first free port from here.
const FALLBACK: std::ops::RangeInclusive<u16> = 10443..=10543;

/// What `tailscale status --json` says about this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub backend_state: String,
    /// The MagicDNS name without its trailing dot (`machine.tailnet.ts.net`).
    pub dns_name: String,
    pub ipv4: Option<String>,
}

impl Status {
    pub fn running(&self) -> bool {
        self.backend_state == "Running"
    }
}

/// An HTTPS mapping that answers for this listener.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exposure {
    pub https_port: u16,
    /// `https://machine.tailnet.ts.net`, with `:port` unless the port is 443.
    pub url: String,
    pub proxy: String,
    /// True when an identical mapping already existed and no command was run.
    pub reused: bool,
}

/// Why the mapping is or is not there, as the dashboard reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// The background check has not finished yet.
    Checking,
    Exposed,
    /// `HORON_TAILSCALE_HTTPS=off`.
    Off,
    /// No `tailscale` CLI on this machine.
    Unavailable,
    /// The CLI is there but the node is not logged in or not running.
    NotConnected,
    /// `server.tls` is on, so a TLS-terminating proxy cannot be used.
    Tls,
    /// The listener has no fixed port to forward to.
    Ephemeral,
    /// The CLI answered, but the mapping could not be made or confirmed.
    Failed,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Checking => "checking",
            State::Exposed => "exposed",
            State::Off => "off",
            State::Unavailable => "unavailable",
            State::NotConnected => "not-connected",
            State::Tls => "tls",
            State::Ephemeral => "ephemeral",
            State::Failed => "failed",
        }
    }
}

/// The last word on the mapping; `detail` is the one-line reason when it is not exposed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub state: State,
    pub url: Option<String>,
    pub https_port: Option<u16>,
    pub detail: Option<String>,
}

impl Default for Report {
    fn default() -> Self {
        Self {
            state: State::Checking,
            url: None,
            https_port: None,
            detail: None,
        }
    }
}

impl Report {
    fn skipped(state: State, detail: impl Into<String>) -> Self {
        Self {
            state,
            detail: Some(detail.into()),
            ..Self::default()
        }
    }

    fn exposed(exposure: &Exposure) -> Self {
        Self {
            state: State::Exposed,
            url: Some(exposure.url.clone()),
            https_port: Some(exposure.https_port),
            detail: None,
        }
    }
}

/// The report shared by the startup task and the management API.
pub type Shared = std::sync::Arc<std::sync::Mutex<Report>>;

pub fn shared() -> Shared {
    std::sync::Arc::new(std::sync::Mutex::new(Report::default()))
}

/// Reads the parts of `tailscale status --json` this needs. A missing MagicDNS name is
/// an error: without it there is no certificate name to publish.
pub fn parse_status(json: &str) -> Result<Status, String> {
    let doc: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("unreadable `tailscale status` output: {e}"))?;
    let backend_state = doc
        .get("BackendState")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let dns_name = doc
        .pointer("/Self/DNSName")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim_end_matches('.')
        .to_owned();
    if dns_name.is_empty() {
        return Err("`tailscale status` reports no MagicDNS name for this machine".to_owned());
    }
    let ipv4 = doc
        .get("TailscaleIPs")
        .and_then(serde_json::Value::as_array)
        .and_then(|ips| {
            ips.iter()
                .filter_map(serde_json::Value::as_str)
                .find(|ip| ip.parse::<Ipv4Addr>().is_ok())
        })
        .map(str::to_owned);
    Ok(Status {
        backend_state,
        dns_name,
        ipv4,
    })
}

/// Maps each HTTPS port of `tailscale serve status --json` to its `/` proxy target. A
/// web entry without a port suffix is the default 443.
pub fn parse_serve_ports(json: &str) -> Result<BTreeMap<u16, String>, String> {
    let doc: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("unreadable `tailscale serve status` output: {e}"))?;
    let mut ports = BTreeMap::new();
    let Some(web) = doc.get("Web").and_then(serde_json::Value::as_object) else {
        return Ok(ports);
    };
    for (host, entry) in web {
        let port = match host.rsplit_once(':') {
            Some((_, port)) => port.parse::<u16>().unwrap_or(443),
            None => 443,
        };
        // The root handler's key is `/`, escaped as `~1` in a JSON pointer.
        if let Some(proxy) = entry.pointer("/Handlers/~1/Proxy").and_then(serde_json::Value::as_str) {
            ports.insert(port, proxy.to_owned());
        }
    }
    Ok(ports)
}

/// The port to publish on: the pinned one, else the port that already serves `proxy`,
/// else the first free candidate, else the first free fallback port.
pub fn choose_port(preferred: Option<u16>, taken: &BTreeMap<u16, String>, proxy: &str) -> Option<u16> {
    if let Some(port) = preferred {
        return Some(port);
    }
    if let Some((&port, _)) = taken.iter().find(|(_, target)| target.as_str() == proxy) {
        return Some(port);
    }
    if let Some(port) = CANDIDATES.iter().copied().find(|port| !taken.contains_key(port)) {
        return Some(port);
    }
    FALLBACK.into_iter().find(|port| !taken.contains_key(port))
}

/// The local URL `tailscale serve` forwards to. A wildcard bind is reachable on loopback;
/// any other bind is forwarded to the address it actually uses.
pub fn proxy_target(bound: SocketAddr) -> String {
    let ip = match bound.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        ip => ip,
    };
    match ip {
        IpAddr::V4(ip) => format!("http://{ip}:{}", bound.port()),
        IpAddr::V6(ip) => format!("http://[{ip}]:{}", bound.port()),
    }
}

/// Whether the automatic exposure is on. Any value but an off spelling leaves it on, so
/// an empty or unrelated value never silently disables it.
pub fn enabled(get: impl Fn(&str) -> Option<String>) -> bool {
    let value = get(ENV_ENABLE).map(|v| v.trim().to_ascii_lowercase());
    !matches!(
        value.as_deref(),
        Some("off" | "0" | "false" | "no" | "disable" | "disabled")
    )
}

/// The pinned HTTPS port, if the environment names a usable one.
pub fn https_port_override(get: impl Fn(&str) -> Option<String>) -> Option<u16> {
    get(ENV_PORT)
        .and_then(|v| v.trim().parse::<u16>().ok())
        .filter(|port| *port != 0)
}

/// The `tailscale` CLI, with every call bounded by [`TIMEOUT`].
#[derive(Debug, Clone)]
pub struct Client {
    bin: PathBuf,
}

impl Client {
    pub fn new(bin: PathBuf) -> Self {
        Self { bin }
    }

    /// The CLI named by `HORON_TAILSCALE_BIN`, then the first `tailscale` on `PATH`, then
    /// the install locations the GUI apps use.
    pub fn discover() -> Option<Self> {
        let env = |key: &str| std::env::var(key).ok();
        if let Some(path) = env(ENV_BIN).map(PathBuf::from).filter(|path| path.is_file()) {
            return Some(Self::new(path));
        }
        if let Some(path) = env("PATH").and_then(|path| which(&path)) {
            return Some(Self::new(path));
        }
        KNOWN
            .iter()
            .map(PathBuf::from)
            .find(|path| path.is_file())
            .map(Self::new)
    }

    async fn run(&self, args: &[&str]) -> Result<String, String> {
        let mut command = tokio::process::Command::new(&self.bin);
        command.args(args).stdin(std::process::Stdio::null()).kill_on_drop(true);
        let output = match tokio::time::timeout(TIMEOUT, command.output()).await {
            Ok(Ok(output)) => output,
            Ok(Err(error)) => return Err(format!("{} {}: {error}", self.bin.display(), args.join(" "))),
            Err(_) => {
                return Err(format!(
                    "{} {}: no answer within {}s",
                    self.bin.display(),
                    args.join(" "),
                    TIMEOUT.as_secs()
                ));
            }
        };
        if !output.status.success() {
            // The last non-empty stderr line is the reason; earlier ones are version noise.
            let stderr = String::from_utf8_lossy(&output.stderr);
            let reason = stderr
                .lines()
                .rev()
                .find(|line| !line.trim().is_empty())
                .unwrap_or("")
                .trim();
            let reason = if reason.is_empty() {
                format!("exited with {}", output.status)
            } else {
                reason.to_owned()
            };
            return Err(format!("{} {}: {reason}", self.bin.display(), args.join(" ")));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    pub async fn status(&self) -> Result<Status, String> {
        parse_status(&self.run(&["status", "--json"]).await?)
    }

    pub async fn serve_ports(&self) -> Result<BTreeMap<u16, String>, String> {
        parse_serve_ports(&self.run(&["serve", "status", "--json"]).await?)
    }

    /// `tailscale serve --bg --https=<port> <proxy>`: background, so the mapping outlives
    /// this call and is served by tailscaled.
    pub async fn serve_https(&self, port: u16, proxy: &str) -> Result<(), String> {
        self.run(&["serve", "--bg", &format!("--https={port}"), proxy])
            .await
            .map(|_| ())
    }
}

/// `tailscale` on `PATH`, as the current platform spells executables.
fn which(path: &str) -> Option<PathBuf> {
    let name = if cfg!(windows) { "tailscale.exe" } else { "tailscale" };
    std::env::split_paths(path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

#[cfg(target_os = "macos")]
const KNOWN: &[&str] = &[
    "/Applications/Tailscale.app/Contents/MacOS/Tailscale",
    "/opt/homebrew/bin/tailscale",
    "/usr/local/bin/tailscale",
];
#[cfg(target_os = "linux")]
const KNOWN: &[&str] = &["/usr/bin/tailscale", "/usr/local/bin/tailscale", "/usr/sbin/tailscale"];
#[cfg(windows)]
const KNOWN: &[&str] = &["C:\\Program Files\\Tailscale\\tailscale.exe"];
#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
const KNOWN: &[&str] = &[];

/// Publishes `bound` on the tailnet over HTTPS, or says why it did not.
/// `preferred` is the pinned port; `None` picks one that is free.
pub async fn setup(client: &Client, config: &Config, bound: SocketAddr, tls: bool, preferred: Option<u16>) -> Report {
    if tls {
        let detail = "server.tls is on, and `tailscale serve` needs a plain HTTP backend";
        tracing::info!("tailscale: {detail}; skipping the tailnet HTTPS mapping");
        return Report::skipped(State::Tls, detail);
    }
    if config.port == 0 {
        let detail = "the listener port is ephemeral";
        tracing::debug!("tailscale: {detail}; skipping the HTTPS mapping");
        return Report::skipped(State::Ephemeral, detail);
    }
    let status = match client.status().await {
        Ok(status) => status,
        Err(error) => {
            tracing::debug!("tailscale: {error}");
            return Report::skipped(State::NotConnected, error);
        }
    };
    if !status.running() {
        let detail = format!("Tailscale is not connected (state {})", status.backend_state);
        tracing::debug!("tailscale: {detail}; skipping the HTTPS mapping");
        return Report::skipped(State::NotConnected, detail);
    }
    let proxy = proxy_target(bound);
    let taken = match client.serve_ports().await {
        Ok(taken) => taken,
        Err(error) => {
            // An unreadable status is treated as "nothing served yet": the mapping below
            // is additive and verified afterwards, so this cannot remove another app.
            tracing::debug!("tailscale: {error}");
            BTreeMap::new()
        }
    };
    let Some(port) = choose_port(preferred, &taken, &proxy) else {
        let detail = "every HTTPS port this could use already serves another program";
        tracing::warn!("tailscale: {detail}; not exposing {proxy}");
        return Report::skipped(State::Failed, detail);
    };
    let reused = taken.get(&port).is_some_and(|target| target == &proxy);
    if !reused {
        if let Some(target) = taken.get(&port)
            && preferred.is_some()
        {
            tracing::warn!(
                "tailscale: https port {port} already serves {target}; replacing it because {ENV_PORT} asks for it"
            );
        }
        if let Err(error) = client.serve_https(port, &proxy).await {
            tracing::warn!("tailscale: could not expose {proxy}: {error}");
            return Report::skipped(State::Failed, error);
        }
    }
    // Say nothing until tailscaled reports the mapping; a command that "succeeded" is not
    // evidence that the port answers.
    let confirmed = client
        .serve_ports()
        .await
        .map(|ports| ports.get(&port).is_some_and(|target| target == &proxy))
        .unwrap_or(false);
    if !confirmed {
        let detail = format!("`tailscale serve status` does not list https port {port} -> {proxy}");
        tracing::warn!("tailscale: {detail}");
        return Report::skipped(State::Failed, detail);
    }
    let url = if port == 443 {
        format!("https://{}", status.dns_name)
    } else {
        format!("https://{}:{port}", status.dns_name)
    };
    tracing::info!("tailscale: this server answers at {url} (dashboard: {url}/management.html)");
    if !config
        .trusted_proxies
        .iter()
        .any(|proxy| proxy == "127.0.0.1" || proxy == "::1")
        || !config.management.allow_remote
    {
        // Requests now arrive from loopback, so they count as local. That works, but the
        // dashboard sees one client address for the whole tailnet and the local password
        // is accepted from it; the fix is a config line, not something to write here.
        tracing::info!(
            "tailscale: for remote dashboard access with per-client addresses, set \
             server.trusted-proxies: [127.0.0.1, \"::1\"] and management.allow-remote: true"
        );
    }
    Report::exposed(&Exposure {
        https_port: port,
        url,
        proxy,
        reused,
    })
}

/// Starts [`setup`] off the startup path and records the outcome in `report`.
/// `standalone` (the `-tui` embedded server) never publishes: that listener belongs to the
/// TUI's lifetime, not the tailnet.
pub fn spawn(report: Shared, config: Config, bound: SocketAddr, tls: bool, standalone: bool) {
    let record = {
        let report = report.clone();
        move |result: Report| {
            *report.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = result;
        }
    };
    if standalone {
        return record(Report::skipped(State::Off, "the standalone TUI owns this listener"));
    }
    if !enabled(|key| std::env::var(key).ok()) {
        tracing::debug!("tailscale: {ENV_ENABLE} turns the HTTPS mapping off");
        return record(Report::skipped(State::Off, format!("{ENV_ENABLE} turns it off")));
    }
    let Some(client) = Client::discover() else {
        tracing::debug!("tailscale: CLI not found; skipping the HTTPS mapping");
        return record(Report::skipped(
            State::Unavailable,
            "the tailscale CLI is not installed on this computer",
        ));
    };
    let preferred = https_port_override(|key| std::env::var(key).ok());
    tokio::spawn(async move {
        let result = setup(&client, &config, bound, tls, preferred).await;
        *report.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = result;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape tailscaled 1.104 answers with, trimmed to the fields read here.
    const STATUS: &str = r#"{
        "Version": "1.104.1",
        "BackendState": "Running",
        "TailscaleIPs": ["100.126.156.153", "fd7a:115c:a1e0:ab12:4843:cd96:627e:9c99"],
        "Self": {
            "HostName": "Alpox's MacBook",
            "DNSName": "alpoxs-macbook.tailb90ba.ts.net.",
            "OS": "macOS"
        }
    }"#;

    fn cfg(yaml: &str) -> Config {
        Config::parse(yaml).unwrap()
    }

    #[test]
    fn status_is_read_the_way_tailscale_reports_it() {
        let status = parse_status(STATUS).unwrap();
        assert_eq!(status.backend_state, "Running");
        assert!(status.running());
        // The trailing dot of the FQDN is dropped, so it can be used in a URL.
        assert_eq!(status.dns_name, "alpoxs-macbook.tailb90ba.ts.net");
        assert_eq!(status.ipv4.as_deref(), Some("100.126.156.153"));

        let stopped = parse_status(r#"{"BackendState": "NeedsLogin", "Self": {"DNSName": "x.ts.net."}}"#).unwrap();
        assert!(!stopped.running());
        assert_eq!(stopped.ipv4, None);

        // No MagicDNS name means no certificate name: a reason, not a silent skip.
        let error = parse_status(r#"{"BackendState": "Running", "Self": {"DNSName": ""}}"#).unwrap_err();
        assert!(error.contains("MagicDNS"), "{error}");
        assert!(parse_status("not json").is_err());
    }

    #[test]
    fn serve_status_maps_ports_to_proxy_targets() {
        let json = r#"{
            "TCP": {"443": {"HTTPS": true}, "8443": {"HTTPS": true}},
            "Web": {
                "alpoxs-macbook.tailb90ba.ts.net:443": {"Handlers": {"/": {"Proxy": "http://127.0.0.1:6030"}}},
                "alpoxs-macbook.tailb90ba.ts.net:8443": {"Handlers": {"/": {"Proxy": "http://127.0.0.1:7317"}}}
            },
            "AllowFunnel": {"alpoxs-macbook.tailb90ba.ts.net:443": true}
        }"#;
        let ports = parse_serve_ports(json).unwrap();
        assert_eq!(ports.get(&443).map(String::as_str), Some("http://127.0.0.1:6030"));
        assert_eq!(ports.get(&8443).map(String::as_str), Some("http://127.0.0.1:7317"));
        // A key without a port suffix is the default 443, and TCP-only entries are not web.
        let ports =
            parse_serve_ports(r#"{"Web": {"host.ts.net": {"Handlers": {"/": {"Proxy": "http://127.0.0.1:9"}}}}}"#)
                .unwrap();
        assert_eq!(ports.get(&443).map(String::as_str), Some("http://127.0.0.1:9"));
        assert!(parse_serve_ports("{}").unwrap().is_empty());
        assert!(parse_serve_ports("nope").is_err());
    }

    #[test]
    fn a_free_port_is_chosen_without_disturbing_other_apps() {
        let ours = "http://127.0.0.1:8317";
        let mut taken = BTreeMap::new();
        assert_eq!(choose_port(None, &taken, ours), Some(443));
        // 443 and 8443 belong to other programs; the next candidate is used.
        taken.insert(443, "http://127.0.0.1:6030".to_owned());
        taken.insert(8443, "http://127.0.0.1:7317".to_owned());
        assert_eq!(choose_port(None, &taken, ours), Some(10000));
        // An existing mapping of ours is reused wherever it is, so a restart is a no-op.
        taken.insert(10443, ours.to_owned());
        assert_eq!(choose_port(None, &taken, ours), Some(10443));
        // A pinned port wins even when another program holds it.
        assert_eq!(choose_port(Some(443), &taken, ours), Some(443));
        // With every candidate taken, the fallback range is used.
        let mut full: BTreeMap<u16, String> = CANDIDATES.iter().map(|&p| (p, "http://x".to_owned())).collect();
        assert_eq!(choose_port(None, &full, ours), Some(10443));
        full.extend(FALLBACK.map(|p| (p, "http://x".to_owned())));
        assert_eq!(choose_port(None, &full, ours), None);
    }

    #[test]
    fn the_proxy_target_follows_the_bound_address() {
        let target = |addr: &str| proxy_target(addr.parse().unwrap());
        assert_eq!(target("127.0.0.1:8317"), "http://127.0.0.1:8317");
        assert_eq!(target("0.0.0.0:8317"), "http://127.0.0.1:8317");
        assert_eq!(target("[::]:8317"), "http://[::1]:8317");
        assert_eq!(target("[::1]:8317"), "http://[::1]:8317");
        assert_eq!(target("100.126.156.153:8317"), "http://100.126.156.153:8317");
    }

    #[test]
    fn the_environment_can_turn_the_mapping_off_and_pin_the_port() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |key: &str| {
                pairs
                    .iter()
                    .find(|(name, _)| *name == key)
                    .map(|(_, value)| (*value).to_owned())
            }
        };
        assert!(enabled(env(&[])));
        for off in ["off", "OFF", " 0 ", "false", "no", "disabled"] {
            let value: &'static str = off;
            assert!(
                !enabled(move |key: &str| (key == ENV_ENABLE).then(|| value.to_owned())),
                "{off}"
            );
        }
        assert!(enabled(env(&[(ENV_ENABLE, "on")])));
        assert!(enabled(env(&[(ENV_ENABLE, "")])));

        assert_eq!(https_port_override(env(&[])), None);
        assert_eq!(https_port_override(env(&[(ENV_PORT, "8443")])), Some(8443));
        assert_eq!(https_port_override(env(&[(ENV_PORT, " 10443 ")])), Some(10443));
        // An unparsable or zero port falls back to automatic choice.
        assert_eq!(https_port_override(env(&[(ENV_PORT, "0")])), None);
        assert_eq!(https_port_override(env(&[(ENV_PORT, "http")])), None);
        assert_eq!(https_port_override(env(&[(ENV_PORT, "70000")])), None);
    }

    /// A stand-in `tailscale` that answers from files and records the commands it was
    /// given, so the whole setup path runs without a tailnet.
    #[cfg(unix)]
    struct Fake {
        dir: PathBuf,
    }

    #[cfg(unix)]
    impl Fake {
        fn new(state: &str, dns: &str) -> Self {
            use std::os::unix::fs::PermissionsExt;
            let dir = std::env::temp_dir().join(format!(
                "horon-tailscale-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("status.json"),
                format!(
                    r#"{{"BackendState": "{state}", "TailscaleIPs": ["100.64.0.1"], "Self": {{"DNSName": "{dns}."}}}}"#
                ),
            )
            .unwrap();
            std::fs::write(dir.join("serve.json"), r#"{"Web": {}}"#).unwrap();
            let script = dir.join("tailscale");
            std::fs::write(
                &script,
                "#!/bin/sh\n\
                 dir=$(dirname \"$0\")\n\
                 printf '%s\\n' \"$*\" >> \"$dir/calls\"\n\
                 case \"$*\" in\n\
                 'status --json') cat \"$dir/status.json\" ;;\n\
                 'serve status --json') cat \"$dir/serve.json\" ;;\n\
                 'serve --bg --https='*)\n\
                 \x20 port=${3#--https=}\n\
                 \x20 printf '{\"Web\":{\"h.tailnet.ts.net:%s\":{\"Handlers\":{\"/\":{\"Proxy\":\"%s\"}}}}}' \"$port\" \"$4\" > \"$dir/serve.json\"\n\
                 \x20 echo 'Serve started and running in the background.' ;;\n\
                 *) echo \"unexpected: $*\" >&2; exit 1 ;;\n\
                 esac\n",
            )
            .unwrap();
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
            Self { dir }
        }

        fn client(&self) -> Client {
            Client::new(self.dir.join("tailscale"))
        }

        fn calls(&self) -> Vec<String> {
            std::fs::read_to_string(self.dir.join("calls"))
                .unwrap_or_default()
                .lines()
                .map(str::to_owned)
                .collect()
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_connected_tailnet_publishes_the_listener_once() {
        let fake = Fake::new("Running", "machine.tailnet.ts.net");
        let config = cfg("port: 8317\n");
        let bound = "127.0.0.1:8317".parse().unwrap();

        let report = setup(&fake.client(), &config, bound, false, None).await;
        assert_eq!(report.state, State::Exposed);
        assert_eq!(report.https_port, Some(443));
        assert_eq!(report.url.as_deref(), Some("https://machine.tailnet.ts.net"));
        assert_eq!(report.detail, None);
        assert!(
            fake.calls()
                .contains(&"serve --bg --https=443 http://127.0.0.1:8317".to_owned()),
            "{:?}",
            fake.calls()
        );

        // The second start finds its own mapping and runs no serve command.
        let before = fake.calls().len();
        let again = setup(&fake.client(), &config, bound, false, None).await;
        assert_eq!(again.state, State::Exposed);
        assert_eq!(again.https_port, Some(443));
        assert!(
            !fake.calls()[before..].iter().any(|call| call.starts_with("serve --bg")),
            "{:?}",
            &fake.calls()[before..]
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_pinned_port_is_used_as_given() {
        let fake = Fake::new("Running", "machine.tailnet.ts.net");
        let report = setup(
            &fake.client(),
            &cfg("port: 8317\n"),
            "0.0.0.0:8317".parse().unwrap(),
            false,
            Some(10443),
        )
        .await;
        assert_eq!(report.state, State::Exposed);
        assert_eq!(report.https_port, Some(10443));
        assert_eq!(report.url.as_deref(), Some("https://machine.tailnet.ts.net:10443"));
        assert!(
            fake.calls()
                .contains(&"serve --bg --https=10443 http://127.0.0.1:8317".to_owned())
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn nothing_is_published_without_a_connected_tailnet() {
        let fake = Fake::new("NeedsLogin", "machine.tailnet.ts.net");
        let config = cfg("port: 8317\n");
        let bound = "127.0.0.1:8317".parse().unwrap();
        let report = setup(&fake.client(), &config, bound, false, None).await;
        assert_eq!(report.state, State::NotConnected);
        assert_eq!(report.url, None);
        assert!(report.detail.unwrap().contains("NeedsLogin"));
        // It asked once and stopped; no serve command was run.
        assert_eq!(fake.calls(), vec!["status --json".to_owned()]);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn tls_and_ephemeral_ports_are_left_alone() {
        let fake = Fake::new("Running", "machine.tailnet.ts.net");
        let bound = "127.0.0.1:8317".parse().unwrap();
        let tls = setup(&fake.client(), &cfg("port: 8317\n"), bound, true, None).await;
        assert_eq!(tls.state, State::Tls);
        let ephemeral = setup(&fake.client(), &cfg("port: 0\n"), bound, false, None).await;
        assert_eq!(ephemeral.state, State::Ephemeral);
        // Neither case ran the CLI at all.
        assert!(fake.calls().is_empty(), "{:?}", fake.calls());
    }

    /// The dashboard reads the same report the startup task writes; a fresh one says the
    /// check is still running rather than claiming either outcome.
    #[test]
    fn the_shared_report_starts_as_checking() {
        let shared = shared();
        assert_eq!(shared.lock().unwrap().state, State::Checking);
        assert_eq!(State::Checking.as_str(), "checking");
        assert_eq!(State::Exposed.as_str(), "exposed");
        assert_eq!(State::NotConnected.as_str(), "not-connected");
    }
}
