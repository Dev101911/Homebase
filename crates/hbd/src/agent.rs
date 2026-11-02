//! The supervisor: starts each server's `claude remote-control` in a
//! pseudo-terminal, follows its output into the log, notices prompts that need
//! a person, and stops it gently (SIGINT, then SIGTERM after 10 s).

use crate::logfile::LogFile;
use homebase_core::ansi::{LineBuffer, Stripper};
use homebase_core::claude;
use homebase_core::config::{Config, Server, contract_home, is_valid_id};
use homebase_core::prompts;
use homebase_core::protocol::{ActionResult, ErrorCode, PROTOCOL_VERSION, Pong};
use homebase_core::{Paths, Request, Response, ServerState, ServerStatus};
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How long a server must stay up before "starting" becomes "running".
const SETTLE: Duration = Duration::from_secs(3);
/// SIGINT, then SIGTERM after this long, then SIGKILL after the same again.
const STOP_GRACE: Duration = Duration::from_secs(10);
const TAIL_LINES: usize = 40;

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub struct Agent {
    paths: Paths,
    inner: Mutex<Inner>,
    agent_log: Mutex<LogFile>,
    login_path: std::sync::OnceLock<Option<String>>,
}

struct Inner {
    config: Config,
    config_mtime: Option<SystemTime>,
    config_error: Option<String>,
    runtimes: HashMap<String, Runtime>,
}

#[derive(Default)]
struct Runtime {
    state: Option<ServerState>,
    pid: Option<u32>,
    started_at: Option<u64>,
    restarts: u32,
    last_exit: Option<i32>,
    attention: Option<String>,
    tail: VecDeque<String>,
    command: Option<String>,
    /// Bumped on every start, so threads of an old process leave the new one alone.
    run: u64,
    stop_requested: bool,
    restart_requested: bool,
    /// Kept open for the life of the process: closing it hangs up the terminal.
    master: Option<Box<dyn MasterPty + Send>>,
}

impl Runtime {
    fn state(&self) -> ServerState {
        self.state.unwrap_or(ServerState::Stopped)
    }

    fn push_line(&mut self, line: String) {
        if self.tail.len() == TAIL_LINES {
            self.tail.pop_front();
        }
        self.tail.push_back(line);
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn not_found(id: &str) -> Response {
    Response::err(
        ErrorCode::NotFound,
        format!("no server with id '{id}' (see `hb ls`)"),
    )
}

impl Agent {
    pub fn new(paths: Paths) -> Arc<Self> {
        let agent_log = Mutex::new(LogFile::open(&paths.agent_log()));
        let agent = Arc::new(Agent {
            paths,
            inner: Mutex::new(Inner {
                config: Config::default(),
                config_mtime: None,
                config_error: None,
                runtimes: HashMap::new(),
            }),
            agent_log,
            login_path: std::sync::OnceLock::new(),
        });
        agent.refresh_config(&mut lock(&agent.inner));
        agent
    }

    pub fn log(&self, message: &str) {
        lock(&self.agent_log).event(message);
    }

    /// Re-reads servers.json when it changed on disk. An invalid file is
    /// reported and ignored, keeping the last good config.
    fn refresh_config(&self, inner: &mut Inner) {
        let path = self.paths.servers_file();
        let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        if mtime.is_some() && mtime == inner.config_mtime {
            return;
        }
        inner.config_mtime = mtime;
        match Config::load(&path) {
            Ok(c) => {
                inner.config = c;
                inner.config_error = None;
            }
            Err(e) => {
                let msg = format!("{}: {e}; keeping the last good config", path.display());
                self.log(&msg);
                inner.config_error = Some(msg);
            }
        }
    }

    fn save_config(&self, inner: &mut Inner) -> Result<(), Response> {
        let path = self.paths.servers_file();
        if let Some(err) = &inner.config_error {
            return Err(Response::err(
                ErrorCode::Failed,
                format!("won't overwrite servers.json while it's invalid: {err}"),
            ));
        }
        inner.config.save(&path).map_err(|e| {
            Response::err(
                ErrorCode::Failed,
                format!("can't write {}: {e}", path.display()),
            )
        })?;
        inner.config_mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        Ok(())
    }

    pub fn handle(self: &Arc<Self>, request: Request) -> Response {
        match request {
            Request::Ping => Response::ok(Pong {
                version: env!("CARGO_PKG_VERSION").to_string(),
                protocol: PROTOCOL_VERSION,
                pid: std::process::id(),
            }),
            Request::List => {
                let mut inner = lock(&self.inner);
                self.refresh_config(&mut inner);
                let list: Vec<ServerStatus> = inner
                    .config
                    .servers
                    .iter()
                    .map(|s| self.status_of(&inner, s, false))
                    .collect();
                Response::ok(list)
            }
            Request::Status { id } => {
                let mut inner = lock(&self.inner);
                self.refresh_config(&mut inner);
                match inner.config.server(&id) {
                    Some(s) => Response::ok(self.status_of(&inner, s, true)),
                    None => not_found(&id),
                }
            }
            Request::GetConfig => {
                let mut inner = lock(&self.inner);
                self.refresh_config(&mut inner);
                let mut value = serde_json::to_value(&inner.config).unwrap_or_default();
                if let Some(err) = &inner.config_error {
                    value["error"] = serde_json::Value::String(err.clone());
                }
                Response::ok(value)
            }
            Request::Start { ids, all } => self.for_each(ids, all, true, |a, id| a.start(id)),
            Request::Stop { ids, all } => self.for_each(ids, all, false, |a, id| a.stop(id)),
            Request::Restart { ids, all } => {
                if ids.is_empty() && !all {
                    return Response::err(
                        ErrorCode::Invalid,
                        "name the servers to restart, or pass all",
                    );
                }
                self.for_each(ids, all, false, |a, id| a.restart(id))
            }
            Request::Add { server } => self.add(server),
            Request::Edit { server, restart } => self.edit(server, restart),
            Request::Remove { id } => self.remove(&id),
            Request::Shutdown => Response::ok(serde_json::json!({"stopping": true})),
        }
    }

    /// Runs an action per server. No ids and no `all` means the autostart
    /// servers when `default_autostart`, else nothing.
    fn for_each(
        self: &Arc<Self>,
        ids: Vec<String>,
        all: bool,
        default_autostart: bool,
        action: impl Fn(&Arc<Self>, &str) -> Result<(), String>,
    ) -> Response {
        let targets: Vec<String> = {
            let mut inner = lock(&self.inner);
            self.refresh_config(&mut inner);
            if all {
                inner.config.servers.iter().map(|s| s.id.clone()).collect()
            } else if ids.is_empty() && default_autostart {
                inner
                    .config
                    .servers
                    .iter()
                    .filter(|s| s.auto_start)
                    .map(|s| s.id.clone())
                    .collect()
            } else {
                if let Some(missing) = ids.iter().find(|id| inner.config.server(id).is_none()) {
                    return not_found(missing);
                }
                ids
            }
        };
        let results: Vec<ActionResult> = targets
            .iter()
            .map(|id| match action(self, id) {
                Ok(()) => ActionResult {
                    id: id.clone(),
                    ok: true,
                    message: None,
                },
                Err(m) => ActionResult {
                    id: id.clone(),
                    ok: false,
                    message: Some(m),
                },
            })
            .collect();
        Response::ok(results)
    }

    fn status_of(&self, inner: &Inner, server: &Server, with_command: bool) -> ServerStatus {
        let rt = inner.runtimes.get(&server.id);
        let state = rt.map(Runtime::state).unwrap_or(ServerState::Stopped);
        let command = match rt.and_then(|r| r.command.clone()) {
            Some(c) if state.is_live() => Some(c),
            _ if with_command => Some(self.would_run(&inner.config, server)),
            _ => None,
        };
        ServerStatus {
            id: server.id.clone(),
            name: server.name.clone(),
            folder: server.folder.clone(),
            state,
            pid: rt.and_then(|r| r.pid),
            started_at: rt.and_then(|r| r.started_at),
            restarts: rt.map(|r| r.restarts).unwrap_or(0),
            last_exit: rt.and_then(|r| r.last_exit),
            attention: rt.and_then(|r| r.attention.clone()),
            command,
            last_lines: rt
                .map(|r| r.tail.iter().cloned().collect())
                .unwrap_or_default(),
            log_file: self.paths.server_log(&server.id).display().to_string(),
        }
    }

    fn would_run(&self, config: &Config, server: &Server) -> String {
        let program = claude::find_claude(config.claude_path.as_deref())
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "claude".to_string());
        let args = claude::remote_control_args(server, claude::resolve_spawn(server), false);
        claude::display_command(&program, &args)
    }

    fn add(self: &Arc<Self>, mut server: Server) -> Response {
        if !is_valid_id(&server.id) {
            return Response::err(
                ErrorCode::Invalid,
                format!(
                    "id '{}' must be lower-case letters, digits and dashes",
                    server.id
                ),
            );
        }
        let folder = server.folder_path();
        if !folder.is_dir() {
            return Response::err(
                ErrorCode::Invalid,
                format!("folder {} doesn't exist", folder.display()),
            );
        }
        server.folder = contract_home(&folder);
        let mut inner = lock(&self.inner);
        self.refresh_config(&mut inner);
        if inner.config.server(&server.id).is_some() {
            return Response::err(
                ErrorCode::Invalid,
                format!(
                    "a server with id '{}' already exists; pass --id to choose another",
                    server.id
                ),
            );
        }
        inner.config.servers.push(server.clone());
        if let Err(problems) = inner.config.validate() {
            inner.config.servers.pop();
            return Response::err(ErrorCode::Invalid, problems.join("; "));
        }
        if let Err(r) = self.save_config(&mut inner) {
            inner.config.servers.pop();
            return r;
        }
        self.log(&format!(
            "added server '{}' for {}",
            server.id, server.folder
        ));
        Response::ok(server)
    }

    fn edit(self: &Arc<Self>, mut server: Server, restart: bool) -> Response {
        let folder = server.folder_path();
        if !folder.is_dir() {
            return Response::err(
                ErrorCode::Invalid,
                format!("folder {} doesn't exist", folder.display()),
            );
        }
        server.folder = contract_home(&folder);
        let live = {
            let mut inner = lock(&self.inner);
            self.refresh_config(&mut inner);
            let Some(slot) = inner.config.server_mut(&server.id) else {
                return not_found(&server.id);
            };
            let old = std::mem::replace(slot, server.clone());
            if let Err(problems) = inner.config.validate() {
                *inner.config.server_mut(&server.id).expect("just replaced") = old;
                return Response::err(ErrorCode::Invalid, problems.join("; "));
            }
            if let Err(r) = self.save_config(&mut inner) {
                *inner.config.server_mut(&server.id).expect("just replaced") = old;
                return r;
            }
            inner
                .runtimes
                .get(&server.id)
                .map(|r| r.state().is_live())
                .unwrap_or(false)
        };
        self.log(&format!("edited server '{}'", server.id));
        if live && restart {
            let _ = self.restart(&server.id);
        }
        Response::ok(server)
    }

    fn remove(self: &Arc<Self>, id: &str) -> Response {
        {
            let mut inner = lock(&self.inner);
            self.refresh_config(&mut inner);
            if inner.config.server(id).is_none() {
                return not_found(id);
            }
        }
        let _ = self.stop(id);
        if !self.wait_until_stopped(id, STOP_GRACE * 2 + Duration::from_secs(2)) {
            return Response::err(
                ErrorCode::Failed,
                format!("server '{id}' didn't stop; not removed"),
            );
        }
        let mut inner = lock(&self.inner);
        let Some(pos) = inner.config.servers.iter().position(|s| s.id == id) else {
            return not_found(id);
        };
        let removed = inner.config.servers.remove(pos);
        if let Err(r) = self.save_config(&mut inner) {
            inner.config.servers.insert(pos, removed);
            return r;
        }
        inner.runtimes.remove(id);
        drop(inner);
        self.log(&format!("removed server '{id}' (folder left alone)"));
        Response::ok(removed)
    }

    fn wait_until_stopped(&self, id: &str, timeout: Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let live = lock(&self.inner)
                .runtimes
                .get(id)
                .map(|r| r.pid.is_some())
                .unwrap_or(false);
            if !live {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    pub fn start(self: &Arc<Self>, id: &str) -> Result<(), String> {
        let mut inner = lock(&self.inner);
        self.refresh_config(&mut inner);
        let server = inner
            .config
            .server(id)
            .cloned()
            .ok_or_else(|| format!("no server with id '{id}'"))?;
        if let Some(rt) = inner.runtimes.get(id)
            && rt.pid.is_some()
        {
            return Ok(()); // already running
        }
        let claude_path = inner.config.claude_path.clone();
        let mut log = LogFile::open(&self.paths.server_log(id));

        let fail = |inner: &mut Inner, log: &mut LogFile, msg: String| {
            log.event(&msg);
            let rt = inner.runtimes.entry(id.to_string()).or_default();
            rt.state = Some(ServerState::Failed);
            rt.attention = Some(msg.clone());
            msg
        };

        let Some(program) = claude::find_claude(claude_path.as_deref()) else {
            let msg = match claude_path {
                Some(p) => format!("claude not found at {p} (claudePath in servers.json)"),
                None => "claude not found on the login shell's PATH, ~/.local/bin or /opt/homebrew/bin; install Claude Code or set claudePath".to_string(),
            };
            return Err(fail(&mut inner, &mut log, msg));
        };
        let folder = server.folder_path();
        if !folder.is_dir() {
            return Err(fail(
                &mut inner,
                &mut log,
                format!("folder {} doesn't exist", folder.display()),
            ));
        }

        let args = claude::remote_control_args(&server, claude::resolve_spawn(&server), false);
        let command = claude::display_command(&program.display().to_string(), &args);

        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 50,
                cols: 200,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| fail(&mut inner, &mut log, format!("can't open a terminal: {e}")))?;
        let mut cmd = CommandBuilder::new(&program);
        cmd.args(&args);
        cmd.cwd(&folder);
        cmd.env("TERM", "xterm-256color");
        let login_path = self.login_path.get_or_init(claude::login_shell_path);
        cmd.env("PATH", claude::child_path(&program, login_path.as_deref()));
        cmd.env("HOMEBASE_SERVER_ID", id);
        for (k, v) in &server.env {
            cmd.env(k, v);
        }
        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| fail(&mut inner, &mut log, format!("can't start claude: {e}")))?;
        drop(pair.slave);
        let reader = pair.master.try_clone_reader().map_err(|e| {
            fail(
                &mut inner,
                &mut log,
                format!("can't read the terminal: {e}"),
            )
        })?;
        let pid = child.process_id();

        log.event(&format!("starting in {}: {command}", folder.display()));
        let rt = inner.runtimes.entry(id.to_string()).or_default();
        rt.run += 1;
        let run = rt.run;
        rt.state = Some(ServerState::Starting);
        rt.pid = pid;
        rt.started_at = Some(now());
        rt.attention = None;
        rt.tail.clear();
        rt.command = Some(command);
        rt.stop_requested = false;
        rt.restart_requested = false;
        rt.master = Some(pair.master);
        drop(inner);
        self.log(&format!("started '{id}' (pid {})", pid.unwrap_or(0)));

        let log = Arc::new(Mutex::new(log));

        // Output: log it, keep a tail, and watch for prompts.
        {
            let agent = self.clone();
            let id = id.to_string();
            let log = log.clone();
            thread::spawn(move || agent.follow_output(&id, run, reader, &log));
        }
        // Exit.
        {
            let agent = self.clone();
            let id = id.to_string();
            thread::spawn(move || {
                let code = child.wait().map(|s| s.exit_code() as i32).unwrap_or(-1);
                agent.on_exit(&id, run, code, &log);
            });
        }
        // Settle: starting -> running.
        {
            let agent = self.clone();
            let id = id.to_string();
            thread::spawn(move || {
                thread::sleep(SETTLE);
                let mut inner = lock(&agent.inner);
                if let Some(rt) = inner.runtimes.get_mut(&id)
                    && rt.run == run
                    && rt.state == Some(ServerState::Starting)
                {
                    rt.state = Some(ServerState::Running);
                }
            });
        }
        Ok(())
    }

    fn follow_output(
        &self,
        id: &str,
        run: u64,
        mut reader: Box<dyn Read + Send>,
        log: &Mutex<LogFile>,
    ) {
        let mut stripper = Stripper::default();
        let mut lines_buf = LineBuffer::default();
        let mut buf = [0u8; 8192];
        let mut plain = Vec::new();
        let mut lines = Vec::new();
        loop {
            let n = match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            plain.clear();
            lines.clear();
            stripper.feed(&buf[..n], &mut plain);
            lines_buf.feed(&plain, &mut lines);
            {
                let mut log = lock(log);
                for line in &lines {
                    log.write_line(line);
                }
            }
            let partial = lines_buf.partial();
            let found = lines
                .iter()
                .map(String::as_str)
                .chain(std::iter::once(partial.as_str()))
                .find_map(prompts::detect);
            let mut inner = lock(&self.inner);
            let Some(rt) = inner.runtimes.get_mut(id) else {
                break;
            };
            if rt.run != run {
                break;
            }
            for line in lines.drain(..) {
                if !line.is_empty() {
                    rt.push_line(line);
                }
            }
            if let Some(attention) = found
                && rt.attention.is_none()
            {
                rt.attention = Some(attention.message().to_string());
                rt.state = Some(ServerState::NeedsAttention);
                drop(inner);
                self.log(&format!("'{id}' needs attention: {}", attention.message()));
            }
        }
        let rest = lines_buf.take();
        if !rest.is_empty() {
            lock(log).write_line(&rest);
        }
    }

    fn on_exit(self: &Arc<Self>, id: &str, run: u64, code: i32, log: &Mutex<LogFile>) {
        lock(log).event(&format!("exited with code {code}"));
        let mut inner = lock(&self.inner);
        let Some(rt) = inner.runtimes.get_mut(id) else {
            return;
        };
        if rt.run != run {
            return;
        }
        rt.pid = None;
        rt.master = None;
        rt.last_exit = Some(code);
        let restart = rt.restart_requested;
        if rt.stop_requested || restart {
            rt.state = Some(ServerState::Stopped);
            rt.attention = None;
        } else {
            // Restart policies and backoff arrive in milestone 2; for now an
            // unexpected exit is reported and kept, with the last lines.
            rt.state = Some(ServerState::Failed);
            if rt.attention.is_none() {
                rt.attention = Some(format!("exited with code {code}; see `hb logs {id}`"));
            }
        }
        rt.stop_requested = false;
        rt.restart_requested = false;
        drop(inner);
        self.log(&format!("'{id}' exited with code {code}"));
        if restart {
            if let Ok(mut inner) = self.inner.lock()
                && let Some(rt) = inner.runtimes.get_mut(id)
            {
                rt.restarts += 1;
            }
            if let Err(e) = self.start(id) {
                self.log(&format!("restart of '{id}' failed: {e}"));
            }
        }
    }

    pub fn stop(self: &Arc<Self>, id: &str) -> Result<(), String> {
        let mut inner = lock(&self.inner);
        let Some(rt) = inner.runtimes.get_mut(id) else {
            return Ok(()); // never started
        };
        let Some(pid) = rt.pid else {
            rt.state = Some(ServerState::Stopped);
            rt.attention = None;
            return Ok(());
        };
        rt.stop_requested = true;
        let run = rt.run;
        drop(inner);
        self.log(&format!("stopping '{id}' (pid {pid})"));
        signal(pid, libc::SIGINT);
        let agent = self.clone();
        let id = id.to_string();
        thread::spawn(move || {
            for sig in [libc::SIGTERM, libc::SIGKILL] {
                thread::sleep(STOP_GRACE);
                let still_running = lock(&agent.inner)
                    .runtimes
                    .get(&id)
                    .map(|r| r.run == run && r.pid.is_some())
                    .unwrap_or(false);
                if !still_running {
                    return;
                }
                agent.log(&format!("'{id}' still running; sending signal {sig}"));
                signal(pid, sig);
            }
        });
        Ok(())
    }

    /// Stops, then starts once the old process has exited. Returns at once.
    pub fn restart(self: &Arc<Self>, id: &str) -> Result<(), String> {
        let live = {
            let mut inner = lock(&self.inner);
            match inner.runtimes.get_mut(id) {
                Some(rt) if rt.pid.is_some() => {
                    rt.restart_requested = true;
                    true
                }
                _ => false,
            }
        };
        if live { self.stop(id) } else { self.start(id) }
    }

    pub fn stop_all_and_wait(self: &Arc<Self>) {
        let ids: Vec<String> = lock(&self.inner)
            .runtimes
            .iter()
            .filter(|(_, r)| r.pid.is_some())
            .map(|(id, _)| id.clone())
            .collect();
        for id in &ids {
            let _ = self.stop(id);
        }
        for id in &ids {
            self.wait_until_stopped(id, STOP_GRACE + Duration::from_secs(2));
        }
    }
}

/// Signals the process group the terminal session leads (as Ctrl-C would),
/// falling back to the process itself.
fn signal(pid: u32, sig: libc::c_int) {
    let pid = pid as libc::pid_t;
    // SAFETY: kill(2) has no memory-safety preconditions.
    unsafe {
        if libc::kill(-pid, sig) != 0 {
            libc::kill(pid, sig);
        }
    }
}
