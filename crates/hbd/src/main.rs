//! hbd: the Homebase agent. It owns every `claude remote-control` process and
//! answers the menu bar app and `hb` over a Unix socket.

mod agent;
mod logfile;

use agent::Agent;
use homebase_core::{Paths, Request, Response, protocol::ErrorCode};
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

const USAGE: &str = "\
hbd — the Homebase agent

Usage: hbd [--autostart]

  --autostart   start the servers marked autoStart once the agent is up

You rarely run this yourself: `hb` starts it when needed, and the menu bar
app registers it as a login item. Talk to it with `hb`.";

fn main() {
    let mut autostart = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--autostart" => autostart = true,
            "--version" | "-V" => {
                println!("hbd {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--help" | "-h" => {
                println!("{USAGE}");
                return;
            }
            other => {
                eprintln!("hbd: unknown argument '{other}'\n\n{USAGE}");
                std::process::exit(2);
            }
        }
    }

    let paths = Paths::from_env();
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    let code = runtime.block_on(run(paths, autostart));
    std::process::exit(code);
}

async fn run(paths: Paths, autostart: bool) -> i32 {
    let socket = paths.socket();
    if std::os::unix::net::UnixStream::connect(&socket).is_ok() {
        eprintln!("hbd: already running ({})", socket.display());
        return 0;
    }
    if let Err(e) = std::fs::create_dir_all(&paths.data_dir) {
        eprintln!("hbd: can't create {}: {e}", paths.data_dir.display());
        return 1;
    }
    let _ = std::fs::set_permissions(&paths.data_dir, std::fs::Permissions::from_mode(0o700));
    let _ = std::fs::remove_file(&socket);
    let listener = match UnixListener::bind(&socket) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("hbd: can't listen on {}: {e}", socket.display());
            return 1;
        }
    };
    let _ = std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600));

    let agent = Agent::new(paths.clone());
    agent.log(&format!(
        "hbd {} started (pid {})",
        env!("CARGO_PKG_VERSION"),
        std::process::id()
    ));
    if autostart {
        let a = agent.clone();
        tokio::task::spawn_blocking(move || {
            a.handle(Request::Start {
                ids: vec![],
                all: false,
            });
        });
    }

    let (shutdown_tx, mut shutdown_rx) = tokio::sync::mpsc::channel::<()>(1);
    let mut sigterm =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("signal");
    let mut sigint =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt()).expect("signal");

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                if let Ok((stream, _)) = accepted {
                    tokio::spawn(serve(stream, agent.clone(), shutdown_tx.clone()));
                }
            }
            _ = shutdown_rx.recv() => break,
            _ = sigterm.recv() => break,
            _ = sigint.recv() => break,
        }
    }

    let a = agent.clone();
    let _ = tokio::task::spawn_blocking(move || a.stop_all_and_wait()).await;
    let _ = std::fs::remove_file(&socket);
    agent.log("hbd stopped");
    0
}

async fn serve(
    stream: tokio::net::UnixStream,
    agent: Arc<Agent>,
    shutdown: tokio::sync::mpsc::Sender<()>,
) {
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        let (response, stop_agent) = match serde_json::from_str::<Request>(&line) {
            Ok(Request::Shutdown) => (Response::ok(serde_json::json!({"stopping": true})), true),
            Ok(request) => {
                let a = agent.clone();
                let r = tokio::task::spawn_blocking(move || a.handle(request))
                    .await
                    .unwrap_or_else(|e| Response::err(ErrorCode::Failed, e.to_string()));
                (r, false)
            }
            Err(e) => (
                Response::err(ErrorCode::Invalid, format!("bad request: {e}")),
                false,
            ),
        };
        let mut out = serde_json::to_string(&response).unwrap_or_default();
        out.push('\n');
        if write.write_all(out.as_bytes()).await.is_err() {
            break;
        }
        if stop_agent {
            let _ = shutdown.send(()).await;
            break;
        }
    }
}
