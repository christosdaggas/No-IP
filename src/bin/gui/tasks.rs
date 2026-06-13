use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use eframe::egui;

use noip_duc::Notification;

/// Spawn a thread that detects the public IP and sends it back via channel.
pub fn detect_ip(tx: mpsc::Sender<String>, ctx: egui::Context) {
    thread::spawn(move || {
        let result = match detect_ip_blocking() {
            Ok(ip) => ip,
            Err(e) => {
                log::warn!("Public IP detection failed: {e}");
                "unavailable".into()
            }
        };
        if tx.send(result).is_err() {
            log::debug!("detect_ip receiver dropped before result arrived");
        }
        ctx.request_repaint();
    });
}

fn detect_ip_blocking() -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("client build: {e}"))?;
    let resp = client
        .get("http://ip1.dynupdate.no-ip.com")
        .send()
        .map_err(|e| format!("request: {e}"))?;
    let body = resp.text().map_err(|e| format!("body: {e}"))?;
    Ok(body.trim().to_string())
}

/// Fetch hostnames from the No-IP API using a bearer token.
pub fn fetch_hosts_api(api_key: &str) -> Result<Vec<String>, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get("https://api.noip.com/v1/dns/records")
        .bearer_auth(api_key)
        .header("Accept", "application/json")
        .header("User-Agent", "noip-duc-gui/3.3.0")
        .send()
        .map_err(|e| format!("Connection failed: {e}"))?;
    if resp.status().as_u16() == 401 {
        return Err("Invalid API key \u{2014} generate one at my.noip.com/auth/api-keys".into());
    }
    if !resp.status().is_success() {
        return Err(format!("API error HTTP {}", resp.status()));
    }
    let text = resp.text().map_err(|e| e.to_string())?;
    parse_hosts(&text).ok_or_else(|| "No hosts found".into())
}

fn parse_hosts(text: &str) -> Option<Vec<String>> {
    if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(text) {
        let v = names_from(&arr);
        if !v.is_empty() {
            return Some(v);
        }
    }
    if let Ok(obj) = serde_json::from_str::<serde_json::Value>(text) {
        for k in ["records", "dns_records", "data", "hosts", "hostnames"] {
            if let Some(arr) = obj.get(k).and_then(|v| v.as_array()) {
                let v = names_from(arr);
                if !v.is_empty() {
                    return Some(v);
                }
            }
        }
    }
    None
}

fn names_from(arr: &[serde_json::Value]) -> Vec<String> {
    arr.iter()
        .filter_map(|v| {
            if let Some(s) = v.as_str() {
                return Some(s.to_string());
            }
            v.get("hostname")
                .or(v.get("host"))
                .or(v.get("fqdn"))
                .or(v.get("name"))
                .and_then(|s| s.as_str())
                .map(|s| s.to_string())
        })
        .collect()
}

/// Bridge: forwards `Notification` values from the library updater to an mpsc channel.
#[derive(Clone)]
pub struct ChannelObs(pub mpsc::Sender<Notification>);

impl noip_duc::Observer for ChannelObs {
    fn notify(&self, n: Notification) {
        if self.0.send(n).is_err() {
            log::debug!("notification receiver dropped");
        }
    }
}
