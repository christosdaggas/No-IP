use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc;
use std::thread;

use eframe::egui;

/// Identifies which service operation produced an [`OpResult`]. Used by the
/// GUI to drive button-disabled / spinner state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Install,
    Uninstall,
    Start,
    Stop,
    Enable,
    Disable,
    UpdateConfig,
}

impl Op {
    pub fn label(self) -> &'static str {
        match self {
            Op::Install => "Installing service\u{2026}",
            Op::Uninstall => "Removing service\u{2026}",
            Op::Start => "Starting service\u{2026}",
            Op::Stop => "Stopping service\u{2026}",
            Op::Enable => "Enabling autostart\u{2026}",
            Op::Disable => "Disabling autostart\u{2026}",
            Op::UpdateConfig => "Updating service config\u{2026}",
        }
    }
}

/// Result of an asynchronously-launched service operation.
#[derive(Debug)]
pub struct OpResult {
    pub op: Op,
    pub result: Result<(), String>,
}

/// Run a service operation on a worker thread. The result is posted on `tx`
/// and `ctx.request_repaint()` wakes the UI when it arrives. The GUI is
/// expected to track an in-flight `Op` and disable affected buttons until it
/// drains the result.
pub fn spawn_op(
    op: Op,
    args: OpArgs,
    tx: mpsc::Sender<OpResult>,
    ctx: egui::Context,
) {
    thread::spawn(move || {
        let result = match op {
            Op::Install | Op::UpdateConfig => {
                let OpArgs { username, password, hostnames } = args;
                svc_install(&username, &password, &hostnames)
            }
            Op::Uninstall => svc_uninstall(),
            Op::Start => svc_start(),
            Op::Stop => svc_stop(),
            Op::Enable => svc_enable(),
            Op::Disable => svc_disable(),
        };
        if tx.send(OpResult { op, result }).is_err() {
            log::debug!("Service op result channel closed before send");
        }
        ctx.request_repaint();
    });
}

/// Inputs an [`Op`] may need. Empty strings are ignored by ops that don't
/// require them.
#[derive(Default, Clone)]
pub struct OpArgs {
    pub username: String,
    pub password: String,
    pub hostnames: String,
}

/// Path to the user systemd service file.
fn svc_file_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".config/systemd/user/noip-duc.service"))
}

/// Path to the service environment file (credentials + hostnames).
fn svc_env_path() -> Option<PathBuf> {
    let mut p = dirs::config_dir()?;
    p.push("noip-duc");
    if let Err(e) = fs::create_dir_all(&p) {
        log::warn!("Failed to create {}: {e}", p.display());
        return None;
    }
    p.push("service.env");
    Some(p)
}

/// Locate the `noip-duc` CLI binary — first next to ourselves, then on PATH.
fn find_duc_binary() -> Option<String> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("noip-duc");
            if candidate.exists() {
                return candidate.to_str().map(|s| s.to_string());
            }
        }
    }
    let out = Command::new("which").arg("noip-duc").output().ok()?;
    if out.status.success() {
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !p.is_empty() {
            return Some(p);
        }
    }
    None
}

/// Run a `systemctl --user` subcommand and return an error on failure.
fn systemctl(args: &[&str]) -> Result<(), String> {
    let out = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(())
}

pub fn svc_install(username: &str, password: &str, hostnames: &str) -> Result<(), String> {
    let bin = find_duc_binary()
        .ok_or_else(|| "noip-duc binary not found. Build it with:\n  cargo build --bin noip-duc".to_string())?;
    let env_path = svc_env_path().ok_or("Cannot determine config directory")?;
    let svc_path = svc_file_path().ok_or("Cannot determine systemd user directory")?;

    if let Some(p) = svc_path.parent() {
        fs::create_dir_all(p).map_err(|e| format!("Failed to create systemd dir: {e}"))?;
    }

    let env_content = format!(
        "NOIP_USERNAME={username}\nNOIP_PASSWORD={password}\nNOIP_HOSTNAMES={hostnames}\n"
    );
    fs::write(&env_path, &env_content).map_err(|e| format!("Failed to write env file: {e}"))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = fs::set_permissions(&env_path, fs::Permissions::from_mode(0o600)) {
            log::warn!("Failed to chmod 0600 on {}: {e}", env_path.display());
        }
    }

    // User-service template: runs as the invoking user (no DynamicUser),
    // but applies every sandboxing knob systemd offers a user unit.
    let svc_content = format!(
        "[Unit]\n\
         Description=No-IP Dynamic DNS Update Client\n\
         After=network-online.target\n\
         Wants=network-online.target\n\n\
         [Service]\n\
         Type=simple\n\
         EnvironmentFile={env}\n\
         ExecStart={bin}\n\
         Restart=on-failure\n\
         RestartSec=30\n\
         \n\
         # Hardening\n\
         NoNewPrivileges=true\n\
         ProtectSystem=strict\n\
         ProtectHome=read-only\n\
         ProtectKernelTunables=true\n\
         ProtectKernelModules=true\n\
         ProtectKernelLogs=true\n\
         ProtectControlGroups=true\n\
         ProtectClock=true\n\
         ProtectHostname=true\n\
         PrivateTmp=true\n\
         PrivateDevices=true\n\
         LockPersonality=true\n\
         RestrictAddressFamilies=AF_INET AF_INET6\n\
         RestrictNamespaces=true\n\
         RestrictRealtime=true\n\
         RestrictSUIDSGID=true\n\
         MemoryDenyWriteExecute=true\n\
         SystemCallArchitectures=native\n\
         SystemCallFilter=@system-service\n\
         SystemCallFilter=~@privileged @resources\n\
         UMask=0077\n\
         MemoryMax=64M\n\
         TasksMax=16\n\
         CPUQuota=10%%\n\n\
         [Install]\n\
         WantedBy=default.target\n",
        env = env_path.display(),
        bin = bin,
    );
    fs::write(&svc_path, svc_content).map_err(|e| format!("Failed to write service file: {e}"))?;

    systemctl(&["daemon-reload"])
}

pub fn svc_uninstall() -> Result<(), String> {
    let _ = systemctl(&["stop", "noip-duc.service"]);
    let _ = systemctl(&["disable", "noip-duc.service"]);
    if let Some(p) = svc_file_path() { let _ = fs::remove_file(p); }
    if let Some(p) = svc_env_path() { let _ = fs::remove_file(p); }
    systemctl(&["daemon-reload"])
}

pub fn svc_start() -> Result<(), String> {
    systemctl(&["start", "noip-duc.service"])
}

pub fn svc_stop() -> Result<(), String> {
    systemctl(&["stop", "noip-duc.service"])
}

pub fn svc_enable() -> Result<(), String> {
    systemctl(&["enable", "noip-duc.service"])
}

pub fn svc_disable() -> Result<(), String> {
    systemctl(&["disable", "noip-duc.service"])
}

/// Query systemd for service status, returning `(installed, running, enabled)`.
pub fn svc_status() -> (bool, bool, bool) {
    let installed = svc_file_path().map_or(false, |p| p.exists());
    let running = Command::new("systemctl")
        .args(["--user", "is-active", "--quiet", "noip-duc.service"])
        .status()
        .map_or(false, |s| s.success());
    let enabled = Command::new("systemctl")
        .args(["--user", "is-enabled", "--quiet", "noip-duc.service"])
        .status()
        .map_or(false, |s| s.success());
    (installed, running, enabled)
}
