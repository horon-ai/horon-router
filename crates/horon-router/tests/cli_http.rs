//! The built binary's own HTTP surface with no plugins anywhere: startup wiring (config
//! load, credentials, listener) plus the ordinary client-key and route answers that the
//! removed `plugin_wiring` fixture had recorded from Go at 6fecc6e and that passed at the
//! base commit (`GET /healthz`, missing/invalid/valid client keys, an unknown model, an
//! unknown path). It keeps the plugin-independent half of that fixture; the plugin half
//! (frontend-auth providers, interceptors, executors) went with the feature.
//!
//! Everything is local: the config uses `port: 0`, the auth directory lives in the same
//! scratch directory, and the requests go to loopback. Readiness is the binary's own
//! `listening` log line, awaited through a channel with a deadline, so the test never
//! sleeps or polls for the server to come up.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// One recorded request/response pair, headers as the client sends them.
struct Case {
    method: &'static str,
    path: &'static str,
    headers: &'static [(&'static str, &'static str)],
    body: &'static str,
    status: u16,
    content_type: &'static str,
    response: &'static str,
}

/// The plugin-independent requests of the removed fixture, with its recorded answers.
const CASES: [Case; 9] = [
    Case {
        method: "GET",
        path: "/healthz",
        headers: &[],
        body: "",
        status: 200,
        content_type: "application/json; charset=utf-8",
        response: r#"{"status":"ok"}"#,
    },
    Case {
        method: "GET",
        path: "/v1/models",
        headers: &[],
        body: "",
        status: 401,
        content_type: "application/json; charset=utf-8",
        response: r#"{"error":"Missing API key"}"#,
    },
    Case {
        method: "GET",
        path: "/v1/models",
        headers: &[("Authorization", "Bearer k1")],
        body: "",
        status: 200,
        content_type: "application/json; charset=utf-8",
        response: r#"{"data":[],"object":"list"}"#,
    },
    Case {
        method: "GET",
        path: "/v1/models?key=k1",
        headers: &[],
        body: "",
        status: 200,
        content_type: "application/json; charset=utf-8",
        response: r#"{"data":[],"object":"list"}"#,
    },
    Case {
        method: "GET",
        path: "/v1/models",
        headers: &[("Authorization", "Bearer wrong")],
        body: "",
        status: 401,
        content_type: "application/json; charset=utf-8",
        response: r#"{"error":"Invalid API key"}"#,
    },
    // An arbitrary query string (repeated keys, an escape that is not one, a semicolon and
    // a plus) does not change the answer.
    Case {
        method: "GET",
        path: "/v1/models?x=1&y=a%20b&y=2&bad=%zz&semi=1;2&plus=a+b",
        headers: &[("Authorization", "Bearer k1")],
        body: "",
        status: 200,
        content_type: "application/json; charset=utf-8",
        response: r#"{"data":[],"object":"list"}"#,
    },
    Case {
        method: "POST",
        path: "/v1/realtime/client_secrets",
        headers: &[("Authorization", "Bearer wrong")],
        body: "{}",
        status: 401,
        content_type: "application/json; charset=utf-8",
        response: r#"{"error":{"code":"invalid_api_key","message":"Invalid API key","param":null,"type":"authentication_error"}}"#,
    },
    Case {
        method: "POST",
        path: "/v1/chat/completions",
        headers: &[("Authorization", "Bearer k1"), ("Content-Type", "application/json")],
        body: r#"{"model":"no-such-model","messages":[{"role":"user","content":"hi"}]}"#,
        status: 400,
        content_type: "application/json",
        response: r#"{"error":{"message":"unknown provider for model no-such-model","type":"invalid_request_error","code":"model_not_found","param":"model"}}"#,
    },
    Case {
        method: "GET",
        path: "/v1beta/models/gem%69ni-x%2Fy",
        headers: &[("Authorization", "Bearer k1")],
        body: "",
        status: 404,
        content_type: "application/json; charset=utf-8",
        response: r#"{"error":{"message":"Not Found","type":"not_found"}}"#,
    },
];

/// A scratch directory whose removal is the test's cleanup.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let dir = std::env::temp_dir().join(format!("horonrouter-{name}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The running binary; dropping it ends the process.
struct Server {
    child: Child,
    port: u16,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Starts the binary on an ephemeral port with its own config and auth directory, and
/// waits for the `listening` line it logs.
fn start_server(name: &str) -> (Server, Scratch) {
    let scratch = Scratch::new(name);
    let dir = scratch.0.clone();
    std::fs::create_dir_all(dir.join("auth")).unwrap();
    let config = dir.join("config.yaml");
    std::fs::write(
        &config,
        "config-version: 8\nhost: 127.0.0.1\nport: 0\nauth-dir: ./auth\napi-keys:\n  - \"k1\"\nremote-management:\n  disable-control-panel: true\n",
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_horonrouter"))
        .arg("-config")
        .arg(&config)
        .arg("-local-model")
        .current_dir(&dir)
        .env_clear()
        .env("HOME", &dir)
        .env("PATH", "/usr/bin:/bin")
        .env("TZ", "UTC")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // The binary logs where it listens; that line is the readiness event, so the wait is a
    // bounded receive and not a poll.
    let (tx, rx) = mpsc::channel();
    let pipes: Vec<Box<dyn Read + Send>> = vec![
        Box::new(child.stdout.take().unwrap()),
        Box::new(child.stderr.take().unwrap()),
    ];
    for pipe in pipes {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(pipe).lines() {
                match line {
                    Ok(line) => {
                        if tx.send(line).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
    }
    drop(tx);
    let port = listening_port(&rx, Duration::from_secs(30));
    (Server { child, port }, scratch)
}

/// `listening addr=127.0.0.1:<port> tls=false`, or a panic carrying the output seen so far.
fn listening_port(rx: &mpsc::Receiver<String>, deadline: Duration) -> u16 {
    let started = Instant::now();
    let mut seen: Vec<String> = Vec::new();
    loop {
        let left = deadline.checked_sub(started.elapsed()).unwrap_or_default();
        match rx.recv_timeout(left) {
            Ok(line) => {
                if let Some(rest) = line.split("listening addr=").nth(1)
                    && let Some(port) = rest
                        .split_whitespace()
                        .next()
                        .and_then(|addr| addr.rsplit(':').next())
                        .and_then(|port| port.parse::<u16>().ok())
                {
                    return port;
                }
                seen.push(line);
            }
            Err(e) => panic!("the server logged no `listening` line ({e}): {seen:#?}"),
        }
    }
}

/// One reply, header names lowercased.
struct Reply {
    status: u16,
    headers: BTreeMap<String, String>,
    body: String,
}

impl Reply {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(&name.to_ascii_lowercase()).map(String::as_str)
    }
}

/// One raw HTTP/1.1 request over a fresh connection, as the fixture's generator sent it
/// (`Connection: close`, then read to the end).
fn request(port: u16, case: &Case) -> Reply {
    let mut conn = TcpStream::connect(("127.0.0.1", port)).expect("connect to the binary");
    conn.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    let mut head = format!("{} {} HTTP/1.1\r\nHost: horonrouter.test\r\n", case.method, case.path);
    for (name, value) in case.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    if !case.body.is_empty() || case.method == "POST" {
        head.push_str(&format!("Content-Length: {}\r\n", case.body.len()));
    }
    head.push_str("Connection: close\r\n\r\n");
    conn.write_all(head.as_bytes()).unwrap();
    conn.write_all(case.body.as_bytes()).unwrap();
    let mut raw = Vec::new();
    conn.read_to_end(&mut raw).unwrap();
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("no HTTP head in the reply");
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let body = String::from_utf8_lossy(&raw[split + 4..]).into_owned();
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .and_then(|code| code.parse().ok())
        .expect("no status line in the reply");
    let mut headers = BTreeMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
        }
    }
    assert!(
        !headers.contains_key("transfer-encoding"),
        "unexpected chunked reply: {head}"
    );
    Reply { status, headers, body }
}

/// The headers every route answers with. `Access-Control-Expose-Headers` is checked for the
/// general entries rather than pinned whole: it still carries `X-CPA-SUPPORT-PLUGIN` here,
/// which the server half of this removal drops on purpose.
fn general_headers(reply: &Reply, content_type: &str) {
    assert_eq!(reply.header("content-type"), Some(content_type));
    assert_eq!(reply.header("access-control-allow-origin"), Some("*"));
    assert_eq!(reply.header("access-control-allow-headers"), Some("*"));
    assert_eq!(
        reply.header("access-control-allow-methods"),
        Some("GET, POST, PUT, PATCH, DELETE, OPTIONS")
    );
    let expose = reply
        .header("access-control-expose-headers")
        .expect("Access-Control-Expose-Headers");
    for entry in [
        "X-CPA-VERSION",
        "X-CPA-COMMIT",
        "X-CPA-BUILD-DATE",
        "X-CPA-HOME-VERSION",
        "X-CPA-HOME-BUILD-DATE",
        "X-SERVER-VERSION",
        "X-SERVER-BUILD-DATE",
        "Location",
        "Retry-After",
        "X-Request-Id",
        "OpenAI-Request-Id",
    ] {
        assert!(expose.contains(entry), "{entry} missing from Expose-Headers: {expose}");
    }
}

#[test]
fn ordinary_binary_http_surface_without_plugins() {
    let (mut server, scratch) = start_server("cli-http");
    for case in &CASES {
        let reply = request(server.port, case);
        assert_eq!(
            reply.status, case.status,
            "{} {} -> {} {}",
            case.method, case.path, reply.status, reply.body
        );
        assert_eq!(reply.body, case.response, "{} {}", case.method, case.path);
        general_headers(&reply, case.content_type);
    }
    // The server is still the one this test started, and it shuts down with the child.
    assert!(server.child.try_wait().unwrap().is_none(), "the server exited early");
    drop(server);
    drop(scratch);
}
