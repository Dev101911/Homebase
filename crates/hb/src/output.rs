//! Text for people, JSON for scripts and agents.

use homebase_core::protocol::{ActionResult, ErrorCode, ServerState, ServerStatus};
use serde::Serialize;

pub struct Out {
    pub json: bool,
}

#[derive(Debug)]
pub struct Failure {
    pub exit_code: i32,
    pub code: &'static str,
    pub message: String,
    /// The details were already printed (as JSON), so print nothing more.
    pub silent: bool,
}

impl Failure {
    pub fn error(message: impl Into<String>) -> Self {
        Failure {
            exit_code: 1,
            code: "failed",
            message: message.into(),
            silent: false,
        }
    }
    pub fn usage(message: impl Into<String>) -> Self {
        Failure {
            exit_code: 2,
            code: "invalid",
            message: message.into(),
            silent: false,
        }
    }
    pub fn not_found(id: &str) -> Self {
        Failure {
            exit_code: 3,
            code: "not-found",
            message: format!("no server with id '{id}' (see `hb ls`)"),
            silent: false,
        }
    }
    pub fn from_agent(code: ErrorCode, message: String) -> Self {
        match code {
            ErrorCode::NotFound => Failure {
                exit_code: 3,
                code: "not-found",
                message,
                silent: false,
            },
            ErrorCode::Invalid => Failure {
                exit_code: 2,
                code: "invalid",
                message,
                silent: false,
            },
            ErrorCode::Failed => Failure {
                exit_code: 1,
                code: "failed",
                message,
                silent: false,
            },
        }
    }
}

pub fn uptime(started_at: Option<u64>, state: ServerState) -> String {
    let Some(start) = started_at.filter(|_| state.is_live()) else {
        return "-".into();
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(start);
    let s = now.saturating_sub(start);
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m", s / 60),
        3600..86_400 => format!("{}h {}m", s / 3600, (s % 3600) / 60),
        _ => format!("{}d {}h", s / 86_400, (s % 86_400) / 3600),
    }
}

impl Out {
    pub fn print_json(&self, value: &impl Serialize) {
        println!(
            "{}",
            serde_json::to_string_pretty(value).unwrap_or_default()
        );
    }

    /// Success: the data as JSON, or a sentence for people.
    pub fn done(&self, data: &impl Serialize, text: &str) {
        if self.json {
            self.print_json(data);
        } else {
            println!("{text}");
        }
    }

    pub fn fail(&self, f: &Failure) {
        if f.silent {
            return;
        }
        if self.json {
            println!(
                "{}",
                serde_json::json!({ "ok": false, "error": { "code": f.code, "message": f.message } })
            );
        } else {
            eprintln!("hb: {}", f.message);
        }
    }

    pub fn list(&self, list: &[ServerStatus]) {
        if self.json {
            self.print_json(&list);
            return;
        }
        if list.is_empty() {
            println!("No servers yet. Add one with `hb add <folder>`.");
            return;
        }
        let rows: Vec<[String; 5]> = list
            .iter()
            .map(|s| {
                [
                    s.id.clone(),
                    s.name.clone(),
                    s.state.as_str().to_string(),
                    uptime(s.started_at, s.state),
                    s.folder.clone(),
                ]
            })
            .collect();
        let header = ["ID", "NAME", "STATE", "UPTIME", "FOLDER"].map(String::from);
        let mut widths = [0usize; 5];
        for row in std::iter::once(&header).chain(rows.iter()) {
            for (w, cell) in widths.iter_mut().zip(row) {
                *w = (*w).max(cell.chars().count());
            }
        }
        for row in std::iter::once(&header).chain(rows.iter()) {
            let line: Vec<String> = row
                .iter()
                .zip(widths)
                .enumerate()
                .map(|(i, (c, w))| {
                    if i == 4 {
                        c.clone()
                    } else {
                        format!("{c:<w$}")
                    }
                })
                .collect();
            println!("{}", line.join("  "));
        }
        for s in list.iter().filter(|s| s.attention.is_some()) {
            println!("\n{}: {}", s.id, s.attention.as_deref().unwrap_or_default());
        }
    }

    pub fn status(&self, s: &ServerStatus) {
        if self.json {
            self.print_json(s);
            return;
        }
        println!("{} ({})", s.id, s.name);
        println!("  state:     {}", s.state.as_str());
        if let Some(a) = &s.attention {
            println!("  attention: {a}");
        }
        println!("  folder:    {}", s.folder);
        if let Some(pid) = s.pid {
            println!("  pid:       {pid}");
        }
        println!("  uptime:    {}", uptime(s.started_at, s.state));
        println!("  restarts:  {}", s.restarts);
        if let Some(code) = s.last_exit {
            println!("  last exit: {code}");
        }
        if let Some(c) = &s.command {
            println!("  command:   {c}");
        }
        println!("  log:       {}", s.log_file);
        if !s.last_lines.is_empty() {
            println!("  last output:");
            for line in s.last_lines.iter().rev().take(10).rev() {
                println!("    {line}");
            }
        }
    }

    /// Per-server results; fails (exit 1) if any server failed.
    pub fn actions(&self, results: &[ActionResult], verb: &str) -> Result<(), Failure> {
        if self.json {
            self.print_json(&results);
        } else if results.is_empty() {
            println!("Nothing to do.");
        } else {
            for r in results {
                match &r.message {
                    None => println!("{}: {verb}", r.id),
                    Some(m) => eprintln!("{}: failed: {m}", r.id),
                }
            }
        }
        let failed = results.iter().filter(|r| !r.ok).count();
        if failed > 0 {
            let mut f = Failure::error(format!("{failed} of {} failed", results.len()));
            f.silent = self.json; // the JSON results above already say why
            return Err(f);
        }
        Ok(())
    }
}
