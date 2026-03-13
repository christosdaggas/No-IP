use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Path to the user systemd service file.
fn svc_file_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".config/systemd/user/noip-duc.service"))
}

/// Path to the service environment file (credentials + hostnames).
fn svc_env_path() -> Option<PathBuf> {
    dirs::config_dir().map(|mut p| {
        p.push("noip-duc");
        let _ = fs::create_dir_all(&p);
        p.push("service.env");
        p
    })
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
        let _ = fs::set_permissions(&env_path, fs::Permissions::from_mode(0o600));
    }

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
         RestartSec=30\n\n\
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
