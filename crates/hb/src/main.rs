//! hb: the Homebase command line. Every command takes --json, never prompts,
//! and exits 0 ok, 1 error, 2 bad usage, 3 server not found.

mod doctor;
mod output;
mod service;

use clap::{Args, Parser, Subcommand};
use homebase_core::config::{RestartPolicy, Server, SpawnMode, slugify};
use homebase_core::protocol::{ActionResult, ErrorCode, ServerStatus};
use homebase_core::{Config, Paths, Request, Response, client};
use output::{Failure, Out};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const GUIDE: &str = include_str!("../../../docs/agent-guide.md");
const SKILL: &str = include_str!("../../../docs/skill/SKILL.md");

const AFTER_HELP: &str = "\
Examples:
  hb add ~/Projects/my-app --start      make a folder reachable from the Claude app
  hb ls                                  every server and its state
  hb logs my-app -f                      follow a server's output
  hb doctor                              check claude is installed and signed in

For AI agents: run `hb guide` for the full reference (JSON shapes, exit codes,
safe-use rules), or `hb skill install` to teach Claude Code how to use hb.

Exit codes: 0 ok, 1 error, 2 bad usage, 3 server not found.
Not affiliated with Anthropic. Homebase runs your own `claude remote-control`.";

#[derive(Parser)]
#[command(
    name = "hb",
    version,
    about = "Homebase: run and manage Claude Code Remote Control servers",
    long_about = "Homebase keeps one `claude remote-control` server per folder running, so your projects \
stay reachable from the Claude mobile app and claude.ai/code. hb talks to the Homebase agent (hbd), \
starting it when needed; the menu bar app talks to the same agent.",
    after_help = AFTER_HELP
)]
struct Cli {
    /// Print machine-readable JSON instead of text (errors too, on stdout)
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List servers: id, name, state, uptime, folder
    #[command(visible_alias = "list")]
    Ls,
    /// Create a server for a folder
    Add(AddArgs),
    /// Change a server's options (restarts it if running, unless --no-restart)
    Edit(EditArgs),
    /// Stop and delete a server. The folder is never touched
    #[command(visible_alias = "remove")]
    Rm {
        id: String,
        /// Confirm when this would end the session you're typing in
        #[arg(long)]
        yes: bool,
    },
    /// Start servers; with no ids, the ones marked autostart
    Start {
        ids: Vec<String>,
        #[arg(long, conflicts_with = "ids")]
        all: bool,
    },
    /// Stop servers (SIGINT, then SIGTERM after 10 s)
    Stop {
        #[arg(required_unless_present = "all")]
        ids: Vec<String>,
        #[arg(long, conflicts_with = "ids")]
        all: bool,
        /// Confirm when this would end the session you're typing in
        #[arg(long)]
        yes: bool,
    },
    /// Stop, then start. Returns as soon as the agent has the request
    Restart {
        #[arg(required_unless_present = "all")]
        ids: Vec<String>,
        #[arg(long, conflicts_with = "ids")]
        all: bool,
        /// Confirm when this would end the session you're typing in
        #[arg(long)]
        yes: bool,
    },
    /// One server's state, pid, restarts, last exit and exact command line
    Status { id: String },
    /// Print the config as JSON; with an id, just that server
    Config { id: Option<String> },
    /// Print or follow a server's log
    Logs {
        id: String,
        /// Keep printing new output
        #[arg(short, long)]
        follow: bool,
        /// How many lines from the end to print first
        #[arg(short = 'n', long, default_value_t = 100)]
        lines: usize,
    },
    /// Check claude is found and signed in, the agent, the config and the Mac's sleep settings
    Doctor,
    /// Print the reference for AI agents (commands, JSON shapes, exit codes, safe use)
    Guide,
    /// Install a Claude Code skill that teaches sessions to use hb
    Skill {
        #[command(subcommand)]
        action: SkillCmd,
    },
    /// Start the agent at login, for headless Macs (LaunchAgent)
    Service {
        #[command(subcommand)]
        action: ServiceCmd,
    },
    /// The agent process itself
    #[command(hide = true)]
    Agent {
        #[command(subcommand)]
        action: AgentCmd,
    },
}

#[derive(Subcommand)]
enum SkillCmd {
    /// Write the skill to ~/.claude/skills/homebase (or --project for ./.claude/skills)
    Install {
        /// Install into this folder's .claude/skills instead of your user skills
        #[arg(long)]
        project: bool,
        /// Replace an existing copy
        #[arg(long)]
        force: bool,
    },
    /// Print the skill without installing it
    Print,
}

#[derive(Subcommand)]
enum ServiceCmd {
    /// Write the LaunchAgent and load it, so hbd starts at login and autostart servers come up
    Install,
    /// Unload and delete the LaunchAgent (stops the agent and its servers)
    Uninstall,
    /// Show whether the LaunchAgent is installed and loaded
    Status,
}

#[derive(Subcommand)]
enum AgentCmd {
    /// Is the agent running?
    Status,
    /// Stop the agent and every server it runs
    Stop,
}

#[derive(Args)]
struct ServerFlags {
    /// Spawn mode: auto (worktree for a git repo with a commit, else same-dir), same-dir, worktree, session
    #[arg(long, value_parser = parse_spawn)]
    spawn: Option<SpawnMode>,
    /// Max concurrent sessions (claude's --capacity)
    #[arg(long)]
    capacity: Option<u32>,
    /// Permission mode for spawned sessions: acceptEdits, auto, bypassPermissions, default, dontAsk, plan
    #[arg(long)]
    permission_mode: Option<String>,
    /// Pre-create a session in the folder on start
    #[arg(long, overrides_with = "no_create_session")]
    create_session: bool,
    /// Don't pre-create a session (the default for new servers)
    #[arg(long)]
    no_create_session: bool,
    /// Claude in Chrome for spawned sessions
    #[arg(long, overrides_with = "no_chrome")]
    chrome: bool,
    #[arg(long)]
    no_chrome: bool,
    /// Prefix for auto-generated session names
    #[arg(long)]
    name_prefix: Option<String>,
    /// Restart policy: never, on-failure, always
    #[arg(long, value_parser = parse_restart)]
    restart: Option<RestartPolicy>,
    /// Set an environment variable for claude (repeatable)
    #[arg(long = "env", value_name = "KEY=VALUE", value_parser = parse_env)]
    env: Vec<(String, String)>,
    /// Extra arguments passed to `claude remote-control`, after --
    #[arg(last = true)]
    extra_args: Vec<String>,
}

#[derive(Args)]
struct AddArgs {
    /// The project folder
    folder: PathBuf,
    /// Name shown in the Claude app (default: the folder's name)
    #[arg(long)]
    name: Option<String>,
    /// Lower-case id for hb commands (default: a slug of the name)
    #[arg(long)]
    id: Option<String>,
    /// Don't start this server with `hb start` or at login
    #[arg(long)]
    no_autostart: bool,
    /// Start the server right after adding it
    #[arg(long)]
    start: bool,
    #[command(flatten)]
    flags: ServerFlags,
}

#[derive(Args)]
struct EditArgs {
    id: String,
    #[arg(long)]
    name: Option<String>,
    #[arg(long)]
    folder: Option<PathBuf>,
    #[arg(long, overrides_with = "no_autostart")]
    autostart: bool,
    #[arg(long)]
    no_autostart: bool,
    /// Reset options to claude's default: capacity, permission-mode, chrome, name-prefix, create-session, extra-args, env
    #[arg(long, value_name = "FIELD")]
    clear: Vec<String>,
    /// Save without restarting a running server
    #[arg(long)]
    no_restart: bool,
    #[command(flatten)]
    flags: ServerFlags,
}

fn parse_spawn(s: &str) -> Result<SpawnMode, String> {
    s.parse()
}
fn parse_restart(s: &str) -> Result<RestartPolicy, String> {
    s.parse()
}
fn parse_env(s: &str) -> Result<(String, String), String> {
    let (k, v) = s.split_once('=').ok_or("expected KEY=VALUE")?;
    if k.is_empty() {
        return Err("expected KEY=VALUE".into());
    }
    Ok((k.to_string(), v.to_string()))
}

fn main() {
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            let code = if e.use_stderr() { 2 } else { 0 };
            let _ = e.print();
            std::process::exit(code);
        }
    };
    let out = Out { json: cli.json };
    let paths = Paths::from_env();
    let result = run(cli.command, &paths, &out);
    if let Err(f) = result {
        out.fail(&f);
        std::process::exit(f.exit_code);
    }
}

fn run(command: Cmd, paths: &Paths, out: &Out) -> Result<(), Failure> {
    match command {
        Cmd::Ls => {
            let list: Vec<ServerStatus> = call(paths, &Request::List)?;
            out.list(&list);
        }
        Cmd::Add(args) => add(args, paths, out)?,
        Cmd::Edit(args) => edit(args, paths, out)?,
        Cmd::Rm { id, yes } => {
            guard_host(std::slice::from_ref(&id), false, yes, "removing")?;
            let server: Server = call(paths, &Request::Remove { id })?;
            out.done(
                &server,
                &format!("Removed '{}'. {} was left alone.", server.id, server.folder),
            );
        }
        Cmd::Start { ids, all } => {
            let results: Vec<ActionResult> = call(paths, &Request::Start { ids, all })?;
            out.actions(&results, "started")?;
        }
        Cmd::Stop { ids, all, yes } => {
            guard_host(&ids, all, yes, "stopping")?;
            let results: Vec<ActionResult> = call(paths, &Request::Stop { ids, all })?;
            out.actions(&results, "stopping")?;
        }
        Cmd::Restart { ids, all, yes } => {
            guard_host(&ids, all, yes, "restarting")?;
            let results: Vec<ActionResult> = call(paths, &Request::Restart { ids, all })?;
            out.actions(&results, "restarting")?;
        }
        Cmd::Status { id } => {
            let status: ServerStatus = call(paths, &Request::Status { id })?;
            out.status(&status);
        }
        Cmd::Config { id } => {
            let config: serde_json::Value = call(paths, &Request::GetConfig)?;
            let value = match id {
                None => config,
                Some(id) => config["servers"]
                    .as_array()
                    .and_then(|a| a.iter().find(|s| s["id"] == id.as_str()).cloned())
                    .ok_or_else(|| Failure::not_found(&id))?,
            };
            println!(
                "{}",
                serde_json::to_string_pretty(&value).unwrap_or_default()
            );
        }
        Cmd::Logs { id, follow, lines } => logs(paths, out, &id, follow, lines)?,
        Cmd::Doctor => doctor::run(paths, out)?,
        Cmd::Guide => {
            if out.json {
                println!("{}", serde_json::json!({ "guide": GUIDE }));
            } else {
                print!("{GUIDE}");
            }
        }
        Cmd::Skill { action } => skill(action, out)?,
        Cmd::Service { action } => match action {
            ServiceCmd::Install => service::install(paths, out)?,
            ServiceCmd::Uninstall => service::uninstall(out)?,
            ServiceCmd::Status => service::status(out)?,
        },
        Cmd::Agent { action } => match action {
            AgentCmd::Status => match client::request(&paths.socket(), &Request::Ping) {
                Ok(r) if r.ok => out.done(&r.data, "The agent is running."),
                _ => out.done(
                    &serde_json::json!({"running": false}),
                    "The agent isn't running. Any hb command starts it.",
                ),
            },
            AgentCmd::Stop => match client::request(&paths.socket(), &Request::Shutdown) {
                Ok(_) => out.done(
                    &serde_json::json!({"stopping": true}),
                    "Stopping the agent and its servers.",
                ),
                Err(_) => out.done(
                    &serde_json::json!({"running": false}),
                    "The agent wasn't running.",
                ),
            },
        },
    }
    Ok(())
}

/// Sends a request, starting the agent first if it isn't running.
fn call<T: serde::de::DeserializeOwned>(paths: &Paths, request: &Request) -> Result<T, Failure> {
    let response = send_starting_agent(paths, request)?;
    into_data(response)
}

fn into_data<T: serde::de::DeserializeOwned>(response: Response) -> Result<T, Failure> {
    if !response.ok {
        let e = response
            .error
            .unwrap_or(homebase_core::protocol::ErrorBody {
                code: ErrorCode::Failed,
                message: "the agent returned an error".into(),
            });
        return Err(Failure::from_agent(e.code, e.message));
    }
    serde_json::from_value(response.data.unwrap_or(serde_json::Value::Null))
        .map_err(|e| Failure::error(format!("unexpected reply from the agent: {e}")))
}

fn send_starting_agent(paths: &Paths, request: &Request) -> Result<Response, Failure> {
    let socket = paths.socket();
    if let Ok(r) = client::request(&socket, request) {
        return Ok(r);
    }
    start_agent(paths)?;
    client::request(&socket, request).map_err(|e| {
        Failure::error(format!(
            "can't reach the agent at {}: {e}",
            socket.display()
        ))
    })
}

fn hbd_path() -> PathBuf {
    if let Some(p) = std::env::var_os("HOMEBASE_HBD").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    if let Ok(exe) = std::env::current_exe() {
        let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
        if let Some(dir) = exe.parent() {
            let sibling = dir.join("hbd");
            if sibling.exists() {
                return sibling;
            }
        }
    }
    PathBuf::from("hbd")
}

fn start_agent(paths: &Paths) -> Result<(), Failure> {
    use std::os::unix::process::CommandExt;
    let _ = std::fs::create_dir_all(&paths.log_dir);
    let stderr = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths.log_dir.join("hbd.stderr.log"))
        .map(Stdio::from)
        .unwrap_or_else(|_| Stdio::null());
    let hbd = hbd_path();
    Command::new(&hbd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr)
        .process_group(0)
        .spawn()
        .map_err(|e| Failure::error(format!("can't start the agent ({}): {e}", hbd.display())))?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if client::request(&paths.socket(), &Request::Ping).is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err(Failure::error(format!(
        "started {} but it didn't answer; see {}",
        hbd.display(),
        paths.log_dir.join("hbd.stderr.log").display()
    )))
}

/// A session spawned by a server carries HOMEBASE_SERVER_ID. Stopping that
/// server ends the session, so it needs --yes.
fn guard_host(ids: &[String], all: bool, yes: bool, verb: &str) -> Result<(), Failure> {
    let Ok(host) = std::env::var("HOMEBASE_SERVER_ID") else {
        return Ok(());
    };
    if yes || host.is_empty() || !(all || ids.contains(&host)) {
        return Ok(());
    }
    Err(Failure::usage(format!(
        "this session runs under server '{host}'; {verb} it ends this session. Pass --yes to go ahead"
    )))
}

fn absolute_folder(folder: &Path) -> Result<PathBuf, Failure> {
    let expanded = homebase_core::config::expand_home(&folder.to_string_lossy());
    let abs = std::fs::canonicalize(&expanded)
        .map_err(|e| Failure::usage(format!("folder {}: {e}", expanded.display())))?;
    if !abs.is_dir() {
        return Err(Failure::usage(format!("{} isn't a folder", abs.display())));
    }
    Ok(abs)
}

fn apply_flags(server: &mut Server, f: ServerFlags) {
    if let Some(s) = f.spawn {
        server.spawn = s;
    }
    if let Some(c) = f.capacity {
        server.capacity = Some(c);
    }
    if let Some(m) = f.permission_mode {
        server.permission_mode = Some(m);
    }
    if f.create_session {
        server.create_session_in_dir = Some(true);
    }
    if f.no_create_session {
        server.create_session_in_dir = Some(false);
    }
    if f.chrome {
        server.chrome = Some(true);
    }
    if f.no_chrome {
        server.chrome = Some(false);
    }
    if let Some(p) = f.name_prefix {
        server.name_prefix = Some(p);
    }
    if let Some(r) = f.restart {
        server.restart = r;
    }
    for (k, v) in f.env {
        server.env.insert(k, v);
    }
    if !f.extra_args.is_empty() {
        server.extra_args = f.extra_args;
    }
}

fn add(args: AddArgs, paths: &Paths, out: &Out) -> Result<(), Failure> {
    let folder = absolute_folder(&args.folder)?;
    let name = args.name.unwrap_or_else(|| {
        folder
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Server".into())
    });
    let id = args.id.unwrap_or_else(|| slugify(&name));
    let mut server = Server::new(id, name, folder.display().to_string());
    server.auto_start = !args.no_autostart;
    apply_flags(&mut server, args.flags);
    let server: Server = call(paths, &Request::Add { server })?;
    if args.start {
        let results: Vec<ActionResult> = call(
            paths,
            &Request::Start {
                ids: vec![server.id.clone()],
                all: false,
            },
        )?;
        if let Some(r) = results.iter().find(|r| !r.ok) {
            return Err(Failure::error(format!(
                "added '{}' but it didn't start: {}",
                server.id,
                r.message.clone().unwrap_or_default()
            )));
        }
        out.done(
            &server,
            &format!(
                "Added and started '{}' ({}). Check it with `hb status {}`.",
                server.id, server.name, server.id
            ),
        );
    } else {
        out.done(
            &server,
            &format!(
                "Added '{}' ({}). Start it with `hb start {}`.",
                server.id, server.name, server.id
            ),
        );
    }
    Ok(())
}

fn edit(args: EditArgs, paths: &Paths, out: &Out) -> Result<(), Failure> {
    let config: Config = call(paths, &Request::GetConfig)?;
    let mut server = config
        .server(&args.id)
        .cloned()
        .ok_or_else(|| Failure::not_found(&args.id))?;
    if let Some(n) = args.name {
        server.name = n;
    }
    if let Some(f) = args.folder {
        server.folder = absolute_folder(&f)?.display().to_string();
    }
    if args.autostart {
        server.auto_start = true;
    }
    if args.no_autostart {
        server.auto_start = false;
    }
    for field in &args.clear {
        match field.as_str() {
            "capacity" => server.capacity = None,
            "permission-mode" => server.permission_mode = None,
            "chrome" => server.chrome = None,
            "name-prefix" => server.name_prefix = None,
            "create-session" => server.create_session_in_dir = None,
            "extra-args" => server.extra_args.clear(),
            "env" => server.env.clear(),
            other => {
                return Err(Failure::usage(format!(
                    "can't clear '{other}'; expected capacity, permission-mode, chrome, name-prefix, create-session, extra-args or env"
                )));
            }
        }
    }
    apply_flags(&mut server, args.flags);
    let server: Server = call(
        paths,
        &Request::Edit {
            server,
            restart: !args.no_restart,
        },
    )?;
    out.done(&server, &format!("Saved '{}'.", server.id));
    Ok(())
}

fn logs(paths: &Paths, out: &Out, id: &str, follow: bool, lines: usize) -> Result<(), Failure> {
    use std::io::{Read, Seek, SeekFrom};
    let config = Config::load(&paths.servers_file()).map_err(|e| Failure::error(e.to_string()))?;
    if config.server(id).is_none() {
        return Err(Failure::not_found(id));
    }
    let path = paths.server_log(id);
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let all: Vec<&str> = text.lines().collect();
    let tail = &all[all.len().saturating_sub(lines)..];
    if out.json && !follow {
        println!(
            "{}",
            serde_json::json!({ "id": id, "logFile": path.display().to_string(), "lines": tail })
        );
        return Ok(());
    }
    for line in tail {
        println!("{line}");
    }
    if !follow {
        if all.is_empty() && !out.json {
            eprintln!("(no output yet; the log is {})", path.display());
        }
        return Ok(());
    }
    let mut pos = text.len() as u64;
    let mut partial = String::new();
    loop {
        std::thread::sleep(Duration::from_millis(250));
        let Ok(mut file) = std::fs::File::open(&path) else {
            continue;
        };
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        if len < pos {
            pos = 0; // rotated
        }
        if len == pos {
            continue;
        }
        let _ = file.seek(SeekFrom::Start(pos));
        let mut buf = String::new();
        let _ = file.read_to_string(&mut buf);
        pos += buf.len() as u64;
        partial.push_str(&buf);
        while let Some(i) = partial.find('\n') {
            let line: String = partial.drain(..=i).collect();
            let line = line.trim_end_matches('\n');
            if out.json {
                println!("{}", serde_json::json!({ "id": id, "line": line }));
            } else {
                println!("{line}");
            }
        }
    }
}

fn skill(action: SkillCmd, out: &Out) -> Result<(), Failure> {
    match action {
        SkillCmd::Print => print!("{SKILL}"),
        SkillCmd::Install { project, force } => {
            let base = if project {
                std::env::current_dir().map_err(|e| Failure::error(e.to_string()))?
            } else {
                homebase_core::paths::home_dir()
            };
            let dir = base.join(".claude/skills/homebase");
            let file = dir.join("SKILL.md");
            if file.exists() && !force {
                let current = std::fs::read_to_string(&file).unwrap_or_default();
                if current == SKILL {
                    out.done(
                        &serde_json::json!({"path": file, "changed": false}),
                        &format!("The skill is already installed at {}.", file.display()),
                    );
                    return Ok(());
                }
                return Err(Failure::usage(format!(
                    "{} exists and differs; pass --force to replace it",
                    file.display()
                )));
            }
            std::fs::create_dir_all(&dir).map_err(|e| Failure::error(e.to_string()))?;
            std::fs::write(&file, SKILL).map_err(|e| Failure::error(e.to_string()))?;
            out.done(
                &serde_json::json!({"path": file, "changed": true}),
                &format!("Installed the Homebase skill at {}.", file.display()),
            );
        }
    }
    Ok(())
}
