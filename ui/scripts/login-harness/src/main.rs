//! horonrouter's main() with one difference: Options.login_base points every provider's
//! login endpoints (Claude, Codex, Kimi, Meta) at a local fake. Test tool, never shipped.
//! Build: cp ../../../Cargo.lock . && CARGO_TARGET_DIR=../../../target cargo build
//! Run:   login-harness <config.yaml> http://127.0.0.1:9102   (see scripts/fake-logins.mjs)
use std::net::SocketAddr;
use std::sync::Arc;

use hr_core::config::Config;
use hr_exec::Executors;
use hr_exec::claude::{ClaudeExecutor, DEFAULT_BASE_URL};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let path = std::path::PathBuf::from(args.next().expect("config path"));
    let login_base = args.next().expect("fake login base URL");
    let config = Config::load(&path)?;
    let credentials = hr_core::config::credentials::load(&config);
    let listener = tokio::net::TcpListener::bind((config.host.as_str(), config.port)).await?;
    let executors = Executors {
        claude: ClaudeExecutor::new(DEFAULT_BASE_URL)?,
        codex: hr_exec::codex::CodexExecutor::new()?,
        devices: Default::default(),
        openai: Default::default(),
        google: Default::default(),
    };
    // The deny-all test runtime: a credential that is not local cannot leave the machine.
    let rt = Arc::new(hr_server::testing::runtime(config, credentials, executors));
    hr_server::install_registry(&rt);
    let options = hr_server::management::Options { login_base: Some(login_base), ..Default::default() };
    let management = hr_server::management::Management::with_options(rt.clone(), path, options);
    let _watcher = hr_server::watching::start(&management);
    // As main.rs: management answers what the API routes do not, then CORS and the access log.
    let app = hr_server::app(rt.clone(), hr_server::management::router(management))
        .layer(axum::middleware::from_fn(hr_server::management::cors));
    let app = hr_server::observability::router(&rt, app);
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await?;
    Ok(())
}
