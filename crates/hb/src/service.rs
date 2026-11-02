//! `hb service`: run the agent at login without the menu bar app, for
//! headless Macs managed over SSH. The menu bar app registers the same agent
//! with SMAppService instead.

use crate::output::{Failure, Out};
use homebase_core::Paths;
use std::path::PathBuf;
use std::process::Command;

pub const LABEL: &str = "com.tulipbytestudio.homebase.agent";

fn plist_path() -> PathBuf {
    homebase_core::paths::home_dir().join(format!("Library/LaunchAgents/{LABEL}.plist"))
}

fn domain() -> String {
    // SAFETY: getuid(2) can't fail and touches no memory.
    format!("gui/{}", unsafe { libc::getuid() })
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn plist(hbd: &str, paths: &Paths) -> String {
    let path_env = std::env::var("PATH").unwrap_or_default();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{hbd}</string>
    <string>--autostart</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <dict>
    <key>SuccessfulExit</key>
    <false/>
  </dict>
  <!-- An open Claude Code bug reports Remote Control workers launched by
       launchd freezing while the Mac is idle; Interactive is the workaround. -->
  <key>ProcessType</key>
  <string>Interactive</string>
  <key>EnvironmentVariables</key>
  <dict>
    <key>PATH</key>
    <string>{path}</string>
  </dict>
  <key>StandardErrorPath</key>
  <string>{stderr}</string>
</dict>
</plist>
"#,
        hbd = xml_escape(hbd),
        path = xml_escape(&path_env),
        stderr = xml_escape(&paths.log_dir.join("hbd.stderr.log").display().to_string()),
    )
}

fn loaded() -> bool {
    Command::new("launchctl")
        .args(["print", &format!("{}/{LABEL}", domain())])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn install(paths: &Paths, out: &Out) -> Result<(), Failure> {
    if !cfg!(target_os = "macos") {
        return Err(Failure::error(
            "hb service is macOS-only for now; Windows support is planned",
        ));
    }
    let hbd = crate::hbd_path();
    let hbd = std::fs::canonicalize(&hbd).map_err(|_| {
        Failure::error(format!(
            "can't find hbd ({}); install Homebase first",
            hbd.display()
        ))
    })?;
    // Stop an agent hb started by hand, so launchd owns the only one.
    let _ = homebase_core::client::request(&paths.socket(), &homebase_core::Request::Shutdown);
    std::thread::sleep(std::time::Duration::from_millis(500));

    let file = plist_path();
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| Failure::error(e.to_string()))?;
    }
    std::fs::create_dir_all(&paths.log_dir).map_err(|e| Failure::error(e.to_string()))?;
    if loaded() {
        let _ = Command::new("launchctl")
            .args(["bootout", &format!("{}/{LABEL}", domain())])
            .status();
    }
    std::fs::write(&file, plist(&hbd.display().to_string(), paths))
        .map_err(|e| Failure::error(format!("can't write {}: {e}", file.display())))?;
    let status = Command::new("launchctl")
        .args(["bootstrap", &domain()])
        .arg(&file)
        .status()
        .map_err(|e| Failure::error(format!("can't run launchctl: {e}")))?;
    if !status.success() {
        return Err(Failure::error(format!(
            "launchctl bootstrap failed; the plist is at {}. Is this user logged in to a GUI session?",
            file.display()
        )));
    }
    out.done(
        &serde_json::json!({"installed": true, "plist": file, "label": LABEL}),
        &format!(
            "Installed {}. The agent now starts at login and brings up autostart servers.",
            file.display()
        ),
    );
    Ok(())
}

pub fn uninstall(out: &Out) -> Result<(), Failure> {
    let file = plist_path();
    if loaded() {
        let _ = Command::new("launchctl")
            .args(["bootout", &format!("{}/{LABEL}", domain())])
            .status();
    }
    let existed = file.exists();
    if existed {
        std::fs::remove_file(&file).map_err(|e| Failure::error(e.to_string()))?;
    }
    out.done(
        &serde_json::json!({"installed": false, "removed": existed}),
        if existed {
            "Removed the LaunchAgent."
        } else {
            "The LaunchAgent wasn't installed."
        },
    );
    Ok(())
}

pub fn status(out: &Out) -> Result<(), Failure> {
    let file = plist_path();
    let installed = file.exists();
    let is_loaded = loaded();
    out.done(
        &serde_json::json!({"installed": installed, "loaded": is_loaded, "plist": file, "label": LABEL}),
        &format!(
            "LaunchAgent {}: {}, {}.",
            LABEL,
            if installed { "installed" } else { "not installed" },
            if is_loaded { "loaded" } else { "not loaded" }
        ),
    );
    Ok(())
}
