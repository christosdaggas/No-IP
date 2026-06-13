use std::collections::BTreeMap;
use std::fs;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use eframe::egui;
use serde::{Deserialize, Serialize};

use noip_duc::public_ip::IpMethods;
use noip_duc::{updater, Control, Notification};

// ═══ Brand palette ═══
const GREEN: egui::Color32 = egui::Color32::from_rgb(141, 189, 9);
const RED: egui::Color32 = egui::Color32::from_rgb(215, 58, 58);
const BLUE: egui::Color32 = egui::Color32::from_rgb(56, 130, 216);
const AMBER: egui::Color32 = egui::Color32::from_rgb(230, 168, 28);

// Dark
const D_BG: egui::Color32 = egui::Color32::from_rgb(17, 18, 22);
const D_CARD: egui::Color32 = egui::Color32::from_rgb(24, 26, 32);
const D_BORDER: egui::Color32 = egui::Color32::from_rgb(42, 44, 52);
const D_INPUT: egui::Color32 = egui::Color32::from_rgb(20, 22, 28);
const D_TEXT: egui::Color32 = egui::Color32::from_rgb(225, 228, 235);
const D_MUTED: egui::Color32 = egui::Color32::from_rgb(115, 120, 135);
const D_ROW_ALT: egui::Color32 = egui::Color32::from_rgb(28, 30, 36);

// Light
const L_BG: egui::Color32 = egui::Color32::from_rgb(242, 243, 247);
const L_CARD: egui::Color32 = egui::Color32::from_rgb(255, 255, 255);
const L_BORDER: egui::Color32 = egui::Color32::from_rgb(214, 216, 224);
const L_INPUT: egui::Color32 = egui::Color32::from_rgb(246, 247, 250);
const L_TEXT: egui::Color32 = egui::Color32::from_rgb(24, 26, 34);
const L_MUTED: egui::Color32 = egui::Color32::from_rgb(118, 122, 134);
const L_ROW_ALT: egui::Color32 = egui::Color32::from_rgb(246, 247, 250);

const LOGO_PNG: &[u8] = include_bytes!("../../logo.png");
const LOGO_SM_PNG: &[u8] = include_bytes!("../../logo_64.png");

// ═══ Theme helper ═══
#[derive(Clone, Copy)]
struct Th {
    bg: egui::Color32,
    card: egui::Color32,
    border: egui::Color32,
    input: egui::Color32,
    text: egui::Color32,
    muted: egui::Color32,
    row_alt: egui::Color32,
    dark: bool,
}

impl Th {
    fn new(dark: bool) -> Self {
        if dark {
            Self { bg: D_BG, card: D_CARD, border: D_BORDER, input: D_INPUT, text: D_TEXT, muted: D_MUTED, row_alt: D_ROW_ALT, dark }
        } else {
            Self { bg: L_BG, card: L_CARD, border: L_BORDER, input: L_INPUT, text: L_TEXT, muted: L_MUTED, row_alt: L_ROW_ALT, dark }
        }
    }
}

// ═══ Persistent config ═══
#[derive(Clone, Serialize, Deserialize)]
struct AppConfig {
    username: String,
    #[serde(default)]
    password: String,
    #[serde(default)]
    api_key: String,
    #[serde(default)]
    dark_mode: bool,
    #[serde(default)]
    hosts: BTreeMap<String, bool>,
    #[serde(default)]
    save_password: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self { username: String::new(), password: String::new(), api_key: String::new(), dark_mode: true, hosts: BTreeMap::new(), save_password: false }
    }
}

impl AppConfig {
    fn config_path() -> Option<std::path::PathBuf> {
        let mut p = dirs::config_dir()?;
        p.push("noip-duc");
        let _ = fs::create_dir_all(&p);
        p.push("config.json");
        Some(p)
    }
    fn load() -> Self {
        Self::config_path()
            .and_then(|p| fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }
    fn save(&self) {
        if let Some(p) = Self::config_path() {
            let _ = serde_json::to_string_pretty(self).ok().and_then(|s| fs::write(p, s).ok());
        }
    }
    fn selected_hosts(&self) -> Vec<String> {
        self.hosts.iter().filter_map(|(h, &on)| if on { Some(h.clone()) } else { None }).collect()
    }
}

// ═══ Background tasks ═══
fn detect_ip(tx: mpsc::Sender<String>, ctx: egui::Context) {
    thread::spawn(move || {
        let result = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .ok()
            .and_then(|c| c.get("http://ip1.dynupdate.no-ip.com").send().ok())
            .and_then(|r| r.text().ok())
            .map(|t| t.trim().to_string())
            .unwrap_or_else(|| "unavailable".into());
        let _ = tx.send(result);
        ctx.request_repaint();
    });
}

fn fetch_hosts_api(api_key: &str) -> Result<Vec<String>, String> {
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
        if !v.is_empty() { return Some(v); }
    }
    if let Ok(obj) = serde_json::from_str::<serde_json::Value>(text) {
        for k in ["records", "dns_records", "data", "hosts", "hostnames"] {
            if let Some(arr) = obj.get(k).and_then(|v| v.as_array()) {
                let v = names_from(arr);
                if !v.is_empty() { return Some(v); }
            }
        }
    }
    None
}

fn names_from(arr: &[serde_json::Value]) -> Vec<String> {
    arr.iter().filter_map(|v| {
        if let Some(s) = v.as_str() { return Some(s.to_string()); }
        v.get("hostname").or(v.get("host")).or(v.get("fqdn")).or(v.get("name"))
            .and_then(|s| s.as_str()).map(|s| s.to_string())
    }).collect()
}

// ═══ Texture loader ═══
fn load_tex(ctx: &egui::Context, name: &str, bytes: &[u8]) -> egui::TextureHandle {
    let img = image::load_from_memory(bytes).expect("logo decode");
    let rgba = img.to_rgba8();
    let sz = [rgba.width() as usize, rgba.height() as usize];
    let ci = egui::ColorImage::from_rgba_unmultiplied(sz, &rgba.into_raw());
    ctx.load_texture(name, ci, egui::TextureOptions::LINEAR)
}

// ═══ App types ═══
#[derive(PartialEq)]
enum Screen { Login, Fetching, Dashboard }
enum FetchResult { Ok(Vec<String>), Err(String) }
struct LogEntry { ts: String, msg: String, level: u8 }

fn now_hms() -> String {
    let s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
    format!("{:02}:{:02}:{:02}", (s % 86400) / 3600, (s % 3600) / 60, s % 60)
}

// ═══ App ═══
struct DucApp {
    cfg: AppConfig,
    screen: Screen,
    pw_input: String,
    api_key_input: String,
    login_err: String,
    show_api_key: bool,
    save_pw_checked: bool,
    fetch_rx: Option<mpsc::Receiver<FetchResult>>,
    running: bool,
    ip: String,
    status_text: String,
    next_check: Option<String>,
    logs: Vec<LogEntry>,
    host_input: String,
    notif_rx: Option<mpsc::Receiver<Notification>>,
    ctrl_tx: Option<mpsc::Sender<Control>>,
    ip_rx: Option<mpsc::Receiver<String>>,
    logo: egui::TextureHandle,
    logo_sm: egui::TextureHandle,
    svc_installed: bool,
    svc_running: bool,
    svc_enabled: bool,
    svc_msg: String,
}

impl DucApp {
    fn new(cc: &eframe::CreationContext<'_>, cfg: AppConfig) -> Self {
        apply_visuals(&cc.egui_ctx, cfg.dark_mode);
        let logged_in = !cfg.username.is_empty() && !cfg.password.is_empty();
        let logo = load_tex(&cc.egui_ctx, "logo", LOGO_PNG);
        let logo_sm = load_tex(&cc.egui_ctx, "logo_sm", LOGO_SM_PNG);

        let save_pw = cfg.save_password;
        let mut app = Self {
            pw_input: cfg.password.clone(),
            api_key_input: cfg.api_key.clone(),
            cfg,
            screen: if logged_in { Screen::Dashboard } else { Screen::Login },
            login_err: String::new(),
            show_api_key: false,
            save_pw_checked: save_pw,
            fetch_rx: None,
            running: false,
            ip: "Detecting\u{2026}".into(),
            status_text: "Idle".into(),
            next_check: None,
            logs: Vec::new(),
            host_input: String::new(),
            notif_rx: None,
            ctrl_tx: None,
            ip_rx: None,
            logo,
            logo_sm,
            svc_installed: false,
            svc_running: false,
            svc_enabled: false,
            svc_msg: String::new(),
        };
        if logged_in {
            app.kick_ip(&cc.egui_ctx);
            if !app.cfg.selected_hosts().is_empty() { app.start_updater(); }
        }
        app.refresh_svc();
        app
    }

    fn kick_ip(&mut self, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        self.ip_rx = Some(rx);
        detect_ip(tx, ctx.clone());
    }

    fn do_login(&mut self, ctx: &egui::Context) {
        if self.cfg.username.trim().is_empty() || self.pw_input.trim().is_empty() {
            self.login_err = "Enter both username and password.".into();
            return;
        }
        self.cfg.password = self.pw_input.clone();
        self.cfg.api_key = self.api_key_input.trim().to_string();
        self.cfg.save_password = self.save_pw_checked;
        if !self.save_pw_checked {
            // Don't persist password to disk
            let pw_backup = self.cfg.password.clone();
            self.cfg.password.clear();
            self.cfg.save();
            self.cfg.password = pw_backup;
        } else {
            self.cfg.save();
        }
        self.login_err.clear();
        self.screen = Screen::Fetching;
        self.kick_ip(ctx);

        let api_key = self.cfg.api_key.clone();
        let (tx, rx) = mpsc::channel();
        let ectx = ctx.clone();
        self.fetch_rx = Some(rx);
        thread::spawn(move || {
            let r = if api_key.is_empty() {
                FetchResult::Err("No API key \u{2014} add hosts manually or use an API key.".into())
            } else {
                match fetch_hosts_api(&api_key) {
                    Ok(h) => FetchResult::Ok(h),
                    Err(e) => FetchResult::Err(e),
                }
            };
            let _ = tx.send(r);
            ectx.request_repaint();
        });
    }

    fn start_updater(&mut self) {
        let sel = self.cfg.selected_hosts();
        if sel.is_empty() { self.status_text = "No hosts selected".into(); return; }
        if let Some(tx) = self.ctrl_tx.take() { let _ = tx.send(Control::Quit); }

        let (ntx, nrx) = mpsc::channel();
        let (ctx, crx) = mpsc::channel();
        self.notif_rx = Some(nrx);
        self.ctrl_tx = Some(ctx);
        let user = self.cfg.username.clone();
        let pass = self.cfg.password.clone();
        let n = sel.len();

        thread::spawn(move || {
            let im: IpMethods = "dns,http,http-port-8245".parse().unwrap_or_default();
            let c = noip_duc::Config {
                username: &user, password: &pass, hostnames: Some(&sel),
                check_interval: Duration::from_secs(300),
                http_timeout: Duration::from_secs(10),
                exec_on_change: None, ip_method: &im, once: false,
            };
            let _ = updater(c, ChannelObs(ntx), crx);
        });
        self.running = true;
        self.status_text = "Running".into();
        self.log(0, &format!("Updater started \u{2014} {n} host(s)"));
    }

    fn stop_updater(&mut self) {
        if let Some(tx) = self.ctrl_tx.take() { let _ = tx.send(Control::Quit); }
        self.notif_rx = None;
        self.running = false;
        self.status_text = "Stopped".into();
        self.next_check = None;
        self.log(0, "Updater stopped");
    }

    fn log(&mut self, level: u8, msg: &str) {
        self.logs.push(LogEntry { ts: now_hms(), msg: msg.into(), level });
        if self.logs.len() > 200 { self.logs.remove(0); }
    }

    fn drain(&mut self) {
        if let Some(rx) = &self.ip_rx {
            if let Ok(ip) = rx.try_recv() {
                self.ip = ip.clone();
                self.log(0, &format!("Public IP: {ip}"));
                self.ip_rx = None;
            }
        }
        if let Some(rx) = &self.notif_rx {
            for n in rx.try_iter().collect::<Vec<_>>() {
                match n {
                    Notification::CheckIp => { self.status_text = "Checking IP\u{2026}".into(); }
                    Notification::IpChanged { previous, current } => {
                        self.ip = current.to_string();
                        self.log(0, &format!("IP changed: {previous} \u{2192} {current}"));
                    }
                    Notification::NoUpdateNeeded(ip) => {
                        self.ip = ip.to_string();
                        self.status_text = "Running".into();
                    }
                    Notification::Updated { previous, current } => {
                        self.ip = current.to_string();
                        self.status_text = "Updated".into();
                        self.log(0, &format!("DNS updated: {previous} \u{2192} {current}"));
                    }
                    Notification::UpdateFailed(e) => {
                        self.status_text = "Update failed".into();
                        self.log(2, &format!("{e}"));
                    }
                    Notification::NextCheck(d) => {
                        self.next_check = Some(humantime::format_duration(d).to_string());
                        self.status_text = "Running".into();
                    }
                    Notification::GetIpFailedWillRetry(e, r, d) => {
                        self.log(1, &format!("IP lookup failed ({r}), retry {}: {e}", humantime::format_duration(d)));
                    }
                    Notification::Error(e) => { self.log(2, &e); }
                    Notification::Quitting => { self.running = false; self.status_text = "Stopped".into(); }
                    _ => {}
                }
            }
        }
    }

    fn refresh_svc(&mut self) {
        self.svc_installed = svc_file_path().map(|p| p.exists()).unwrap_or(false);
        if self.svc_installed {
            self.svc_running = std::process::Command::new("systemctl")
                .args(["--user", "is-active", "--quiet", "noip-duc.service"])
                .status().map(|s| s.success()).unwrap_or(false);
            self.svc_enabled = std::process::Command::new("systemctl")
                .args(["--user", "is-enabled", "--quiet", "noip-duc.service"])
                .status().map(|s| s.success()).unwrap_or(false);
        } else {
            self.svc_running = false;
            self.svc_enabled = false;
        }
    }
}

// ═══ eframe::App ═══
impl eframe::App for DucApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain();

        if self.screen == Screen::Fetching {
            if let Some(rx) = &self.fetch_rx {
                if let Ok(r) = rx.try_recv() {
                    match r {
                        FetchResult::Ok(hosts) => {
                            let n = hosts.len();
                            for h in hosts { self.cfg.hosts.entry(h).or_insert(false); }
                            self.cfg.save();
                            self.log(0, &format!("Fetched {n} host(s)"));
                        }
                        FetchResult::Err(e) => { self.log(1, &e); }
                    }
                    self.fetch_rx = None;
                    self.screen = Screen::Dashboard;
                }
            }
        }

        if self.running || self.ip_rx.is_some() {
            ctx.request_repaint_after(Duration::from_secs(1));
        }

        let th = Th::new(self.cfg.dark_mode);

        // ── Top bar ──
        egui::TopBottomPanel::top("topbar")
            .frame(egui::Frame::none()
                .fill(th.card)
                .inner_margin(egui::Margin { left: 16.0, right: 16.0, top: 8.0, bottom: 8.0 })
                .stroke(egui::Stroke::new(1.0, th.border)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::Image::new(&self.logo_sm).fit_to_exact_size(egui::vec2(22.0, 22.0)));
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("No-IP").size(15.0).strong().color(GREEN));
                    ui.label(egui::RichText::new("DUC").size(15.0).strong());
                    ui.label(egui::RichText::new("v3.3").size(9.0).color(th.muted));

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let icon = if th.dark { "\u{2600}" } else { "\u{1F319}" };
                        if theme_btn(ui, icon, &th) {
                            self.cfg.dark_mode = !self.cfg.dark_mode;
                            apply_visuals(ctx, self.cfg.dark_mode);
                            self.cfg.save();
                        }
                        if self.screen == Screen::Dashboard {
                            ui.add_space(10.0);
                            let signout = ui.add(egui::Button::new(
                                egui::RichText::new("Sign out").size(12.5).strong().color(egui::Color32::WHITE)
                            ).fill(RED).rounding(6.0).min_size(egui::vec2(0.0, 30.0)));
                            if signout.clicked() {
                                self.stop_updater();
                                let hosts = self.cfg.hosts.clone();
                                let dark = self.cfg.dark_mode;
                                self.cfg = AppConfig { dark_mode: dark, hosts, ..Default::default() };
                                self.pw_input.clear();
                                self.api_key_input.clear();
                                self.save_pw_checked = false;
                                self.cfg.save();
                                self.ip = "Detecting\u{2026}".into();
                                self.logs.clear();
                                self.screen = Screen::Login;
                            }
                            ui.add_space(10.0);
                            ui.label(egui::RichText::new(&self.cfg.username).size(11.0).color(th.muted));
                        }
                    });
                });
            });

        match self.screen {
            Screen::Login => self.draw_login(ctx, &th),
            Screen::Fetching => self.draw_fetching(ctx, &th),
            Screen::Dashboard => self.draw_dashboard(ctx, &th),
        }
    }
}

// ═══ Screens ═══
impl DucApp {
    fn draw_login(&mut self, ctx: &egui::Context, th: &Th) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(th.bg))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(44.0);
                    ui.add(egui::Image::new(&self.logo).fit_to_exact_size(egui::vec2(80.0, 80.0)));
                    ui.add_space(14.0);
                    ui.label(egui::RichText::new("Welcome to No-IP DUC").size(22.0).strong());
                    ui.add_space(2.0);
                    ui.label(egui::RichText::new("Dynamic DNS Update Client").size(12.0).color(th.muted));
                    ui.add_space(24.0);

                    // Login card
                    egui::Frame::none()
                        .fill(th.card)
                        .rounding(14.0)
                        .inner_margin(egui::Margin::symmetric(32.0, 28.0))
                        .stroke(egui::Stroke::new(1.0, th.border))
                        .show(ui, |ui| {
                            ui.set_max_width(360.0);
                            ui.spacing_mut().item_spacing.y = 6.0;

                            field_label(ui, "Username / Email", th);
                            text_field(ui, &mut self.cfg.username, "you@example.com", th, false);
                            ui.add_space(4.0);

                            field_label(ui, "Password / DDNS Key", th);
                            let pw_r = text_field(ui, &mut self.pw_input, "password", th, true);
                            ui.add_space(4.0);

                            ui.horizontal(|ui| {
                                green_checkbox(ui, &mut self.save_pw_checked, "Remember password");
                            });
                            ui.add_space(4.0);

                            // API key toggle
                            let tog = if self.show_api_key { "v API Key (optional)" } else { "> API Key (optional)" };
                            if ui.add(egui::Button::new(
                                egui::RichText::new(tog).size(11.0).color(th.muted)
                            ).frame(false)).clicked() {
                                self.show_api_key = !self.show_api_key;
                            }

                            if self.show_api_key {
                                ui.add_space(2.0);
                                text_field(ui, &mut self.api_key_input, "paste API key here", th, false);
                                ui.add_space(2.0);
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("\u{2139}").size(11.0).color(BLUE));
                                    ui.label(egui::RichText::new("Get yours at my.noip.com/auth/api-keys").size(10.0).color(BLUE));
                                });
                            }

                            if !self.login_err.is_empty() {
                                ui.add_space(4.0);
                                ui.colored_label(RED, egui::RichText::new(&self.login_err).size(12.0));
                            }

                            ui.add_space(14.0);

                            // Sign in button
                            let prev_pad = ui.spacing().button_padding;
                            ui.spacing_mut().button_padding = egui::vec2(24.0, prev_pad.y);
                            let btn = egui::Button::new(
                                egui::RichText::new("Sign In").size(17.0).strong().color(egui::Color32::WHITE)
                            ).fill(GREEN).rounding(10.0).min_size(egui::vec2(ui.available_width(), 52.0));

                            let enter_pressed = pw_r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                            if ui.add(btn).clicked() || enter_pressed {
                                self.do_login(ctx);
                            }
                            ui.spacing_mut().button_padding = prev_pad;
                        });
                });
            });
    }

    fn draw_fetching(&self, ctx: &egui::Context, th: &Th) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(th.bg))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(100.0);
                    ui.add(egui::Image::new(&self.logo).fit_to_exact_size(egui::vec2(56.0, 56.0)));
                    ui.add_space(18.0);
                    ui.spinner();
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new("Connecting\u{2026}").size(16.0));
                    ui.label(egui::RichText::new("Fetching your hosts").size(12.0).color(th.muted));
                });
            });
        ctx.request_repaint_after(Duration::from_millis(100));
    }

    fn draw_dashboard(&mut self, ctx: &egui::Context, th: &Th) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none()
                .fill(th.bg)
                .inner_margin(egui::Margin::symmetric(18.0, 14.0)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().id_source("dash").show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 14.0;

                    // ════ STATUS CARD ════
                    card(ui, th, |ui: &mut egui::Ui| {
                        // Header row: status badge + action buttons
                        ui.horizontal(|ui: &mut egui::Ui| {
                            // Status badge
                            let (dot_col, label) = if self.running {
                                (GREEN, "Running")
                            } else {
                                (th.muted, "Stopped")
                            };

                            // Draw colored dot
                            let (_, dot_rect) = ui.allocate_space(egui::vec2(10.0, 10.0));
                            ui.painter().circle_filled(dot_rect.center(), 5.0, dot_col);
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new(label).size(15.0).strong().color(dot_col));

                            // Action buttons on right
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui: &mut egui::Ui| {
                                if self.running {
                                    if action_btn(ui, "\u{21BB} Refresh", GREEN) {
                                        if let Some(tx) = &self.ctrl_tx { let _ = tx.send(Control::UpdateNow); }
                                    }
                                    ui.add_space(6.0);
                                    if action_btn(ui, "\u{25A0} Stop", RED) {
                                        self.stop_updater();
                                    }
                                } else {
                                    if action_btn(ui, "\u{25B6} Start", GREEN) {
                                        self.start_updater();
                                    }
                                }
                            });
                        });

                        ui.add_space(12.0);

                        // Status info using Grid for alignment
                        egui::Grid::new("status_grid")
                            .num_columns(2)
                            .spacing([14.0, 8.0])
                            .show(ui, |ui: &mut egui::Ui| {
                                ui.label(egui::RichText::new("IP Address").size(12.0).color(th.muted));
                                ui.label(egui::RichText::new(&self.ip).size(14.0).strong().color(GREEN).monospace());
                                ui.end_row();

                                ui.label(egui::RichText::new("Account").size(12.0).color(th.muted));
                                ui.label(egui::RichText::new(&self.cfg.username).size(13.0));
                                ui.end_row();

                                ui.label(egui::RichText::new("Hosts").size(12.0).color(th.muted));
                                let n = self.cfg.selected_hosts().len();
                                let total = self.cfg.hosts.len();
                                ui.label(egui::RichText::new(format!("{n} of {total} selected")).size(13.0));
                                ui.end_row();

                                if let Some(ref nc) = self.next_check {
                                    ui.label(egui::RichText::new("Next check").size(12.0).color(th.muted));
                                    ui.label(egui::RichText::new(nc).size(13.0).color(th.muted));
                                    ui.end_row();
                                }
                            });
                    });

                    // ════ HOSTS CARD ════
                    card(ui, th, |ui: &mut egui::Ui| {
                        ui.horizontal(|ui: &mut egui::Ui| {
                            ui.label(egui::RichText::new("Hostnames").size(15.0).strong());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui: &mut egui::Ui| {
                                ui.label(egui::RichText::new("select which to update").size(10.0).color(th.muted));
                            });
                        });
                        ui.add_space(8.0);

                        let mut changed = false;
                        let mut remove: Option<String> = None;

                        if self.cfg.hosts.is_empty() {
                            ui.add_space(20.0);
                            ui.vertical_centered(|ui: &mut egui::Ui| {
                                ui.label(egui::RichText::new("No hostnames yet").size(13.0).color(th.muted));
                                ui.add_space(4.0);
                                ui.label(egui::RichText::new("Add one below, or use an API key to auto-detect").size(11.0).color(th.muted));
                            });
                            ui.add_space(20.0);
                        } else {
                            egui::ScrollArea::vertical().id_source("hosts").max_height(180.0).show(ui, |ui: &mut egui::Ui| {
                                let keys: Vec<String> = self.cfg.hosts.keys().cloned().collect();
                                for (i, h) in keys.iter().enumerate() {
                                    let checked = self.cfg.hosts.get_mut(h).unwrap();
                                    let bg = if i % 2 == 1 { th.row_alt } else { egui::Color32::TRANSPARENT };

                                    egui::Frame::none()
                                        .fill(bg)
                                        .rounding(6.0)
                                        .inner_margin(egui::Margin::symmetric(8.0, 5.0))
                                        .show(ui, |ui: &mut egui::Ui| {
                                            ui.horizontal(|ui: &mut egui::Ui| {
                                                if green_checkbox(ui, checked, "") { changed = true; }
                                                ui.add_space(2.0);
                                                ui.label(egui::RichText::new(h).size(13.0).monospace());
                                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui: &mut egui::Ui| {
                                                    if ui.add(
                                                        egui::Button::new(egui::RichText::new("\u{1F5D1}").size(14.0).color(th.muted))
                                                            .frame(false)
                                                    ).on_hover_text("Remove").clicked() {
                                                        remove = Some(h.clone());
                                                    }
                                                });
                                            });
                                        });
                                }
                            });
                        }

                        if let Some(h) = remove { self.cfg.hosts.remove(&h); changed = true; }

                        // Separator
                        ui.add_space(6.0);
                        let r = ui.available_rect_before_wrap();
                        ui.painter().line_segment(
                            [egui::pos2(r.left(), r.top()), egui::pos2(r.right(), r.top())],
                            egui::Stroke::new(1.0, th.border),
                        );
                        ui.add_space(8.0);

                        // Add host row
                        ui.horizontal(|ui: &mut egui::Ui| {
                            let w = ui.available_width() - 120.0;
                            ui.add(
                                egui::TextEdit::singleline(&mut self.host_input)
                                    .desired_width(w)
                                    .hint_text("myhost.ddns.net")
                                    .margin(egui::Margin::symmetric(10.0, 8.0)),
                            );
                            ui.add_space(4.0);
                            if action_btn(ui, "+ Add Host", GREEN) && !self.host_input.trim().is_empty() {
                                self.cfg.hosts.entry(self.host_input.trim().to_string()).or_insert(true);
                                self.host_input.clear();
                                changed = true;
                            }
                        });

                        if changed {
                            self.cfg.save();
                            if self.running { self.stop_updater(); self.start_updater(); }
                        }
                    });

                    // ════ BACKGROUND SERVICE CARD ════
                    card(ui, th, |ui: &mut egui::Ui| {
                        ui.horizontal(|ui: &mut egui::Ui| {
                            ui.label(egui::RichText::new("Background Service").size(15.0).strong());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui: &mut egui::Ui| {
                                let (dot_col, label) = if self.svc_running {
                                    (GREEN, "Active")
                                } else if self.svc_installed {
                                    (AMBER, "Stopped")
                                } else {
                                    (th.muted, "Not installed")
                                };
                                ui.label(egui::RichText::new(label).size(12.0).color(dot_col));
                                ui.add_space(6.0);
                                let (_, dot_r) = ui.allocate_space(egui::vec2(10.0, 10.0));
                                ui.painter().circle_filled(dot_r.center(), 5.0, dot_col);
                            });
                        });
                        ui.add_space(2.0);
                        ui.label(egui::RichText::new("Keep DNS updated even when the GUI is closed").size(11.0).color(th.muted));
                        ui.add_space(10.0);

                        if !self.svc_installed {
                            let hosts = self.cfg.selected_hosts();
                            if hosts.is_empty() {
                                ui.label(egui::RichText::new("Select at least one host before installing.").size(12.0).color(AMBER));
                            } else {
                                if action_btn(ui, "Install Service", GREEN) {
                                    let hostnames = hosts.join(",");
                                    match svc_install(&self.cfg.username, &self.cfg.password, &hostnames) {
                                        Ok(()) => {
                                            self.refresh_svc();
                                            self.log(0, "Background service installed");
                                            self.svc_msg.clear();
                                        }
                                        Err(e) => {
                                            self.log(2, &format!("Service install failed: {e}"));
                                            self.svc_msg = e;
                                        }
                                    }
                                }
                            }
                        } else {
                            ui.horizontal(|ui: &mut egui::Ui| {
                                if self.svc_running {
                                    if action_btn(ui, "Stop", RED) {
                                        match svc_stop() {
                                            Ok(()) => self.svc_msg.clear(),
                                            Err(e) => self.svc_msg = e,
                                        }
                                        self.refresh_svc();
                                    }
                                } else {
                                    if action_btn(ui, "Start", GREEN) {
                                        match svc_start() {
                                            Ok(()) => self.svc_msg.clear(),
                                            Err(e) => self.svc_msg = e,
                                        }
                                        self.refresh_svc();
                                    }
                                }
                                ui.add_space(6.0);
                                if self.svc_enabled {
                                    if action_btn(ui, "Disable Autostart", AMBER) {
                                        let _ = svc_disable();
                                        self.refresh_svc();
                                    }
                                } else {
                                    if action_btn(ui, "Enable Autostart", BLUE) {
                                        let _ = svc_enable();
                                        self.refresh_svc();
                                    }
                                }
                            });
                            ui.add_space(6.0);
                            ui.horizontal(|ui: &mut egui::Ui| {
                                if action_btn(ui, "Update Config", BLUE) {
                                    let hosts = self.cfg.selected_hosts().join(",");
                                    match svc_install(&self.cfg.username, &self.cfg.password, &hosts) {
                                        Ok(()) => {
                                            if self.svc_running {
                                                let _ = svc_stop();
                                                let _ = svc_start();
                                            }
                                            self.refresh_svc();
                                            self.log(0, "Service config updated");
                                            self.svc_msg.clear();
                                        }
                                        Err(e) => self.svc_msg = e,
                                    }
                                }
                                ui.add_space(6.0);
                                if action_btn(ui, "Uninstall", RED) {
                                    let _ = svc_uninstall();
                                    self.refresh_svc();
                                    self.svc_msg.clear();
                                    self.log(0, "Background service removed");
                                }
                            });
                        }

                        if self.running && self.svc_running {
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new("Both the GUI updater and the background service are active.").size(11.0).color(AMBER));
                        }

                        if !self.svc_msg.is_empty() {
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new(&self.svc_msg).size(11.0).color(RED));
                        }
                    });

                    // ════ ACTIVITY LOG ════
                    card(ui, th, |ui: &mut egui::Ui| {
                        ui.label(egui::RichText::new("Activity").size(15.0).strong());
                        ui.add_space(6.0);

                        egui::ScrollArea::vertical()
                            .id_source("actlog")
                            .max_height(150.0)
                            .stick_to_bottom(true)
                            .show(ui, |ui: &mut egui::Ui| {
                                if self.logs.is_empty() {
                                    ui.vertical_centered(|ui: &mut egui::Ui| {
                                        ui.add_space(12.0);
                                        ui.label(egui::RichText::new("No activity yet").color(th.muted));
                                    });
                                }
                                for e in &self.logs {
                                    let (sym, color) = match e.level {
                                        0 => ("\u{2022}", th.text),
                                        1 => ("!!", AMBER),
                                        _ => ("\u{2022}", RED),
                                    };
                                    ui.horizontal(|ui: &mut egui::Ui| {
                                        ui.label(egui::RichText::new(&e.ts).size(11.0).color(th.muted).monospace());
                                        ui.add_space(4.0);
                                        ui.label(egui::RichText::new(sym).size(11.0).color(color));
                                        ui.add_space(2.0);
                                        ui.label(egui::RichText::new(&e.msg).size(12.0).color(color));
                                    });
                                }
                            });
                    });
                }); // ScrollArea
            });
    }
}

// ═══ Reusable components ═══
fn card(ui: &mut egui::Ui, th: &Th, content: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::none()
        .fill(th.card)
        .rounding(12.0)
        .inner_margin(egui::Margin::symmetric(20.0, 16.0))
        .stroke(egui::Stroke::new(1.0, th.border))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            content(ui);
        });
}

fn theme_btn(ui: &mut egui::Ui, icon: &str, th: &Th) -> bool {
    ui.add(egui::Button::new(
        egui::RichText::new(icon).size(14.0)
    ).fill(egui::Color32::TRANSPARENT)
     .stroke(egui::Stroke::new(1.0, th.border))
     .rounding(99.0)
     .min_size(egui::vec2(30.0, 30.0))
    ).clicked()
}

fn green_checkbox(ui: &mut egui::Ui, checked: &mut bool, label: &str) -> bool {
    let desired_size = egui::vec2(16.0, 16.0);
    let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click());
    let toggled = response.clicked();
    if toggled {
        *checked = !*checked;
    }
    let visuals = ui.style().interact(&response);
    let rounding = 3.0;
    if *checked {
        ui.painter().rect_filled(rect, rounding, GREEN);
        // Draw checkmark
        let stroke = egui::Stroke::new(2.0, egui::Color32::BLACK);
        let x0 = rect.left() + 3.5;
        let y0 = rect.center().y;
        let x1 = rect.left() + 6.5;
        let y1 = rect.bottom() - 3.5;
        let x2 = rect.right() - 3.0;
        let y2 = rect.top() + 4.0;
        ui.painter().line_segment([egui::pos2(x0, y0), egui::pos2(x1, y1)], stroke);
        ui.painter().line_segment([egui::pos2(x1, y1), egui::pos2(x2, y2)], stroke);
    } else {
        ui.painter().rect_stroke(rect, rounding, egui::Stroke::new(1.5, visuals.fg_stroke.color));
    }
    if !label.is_empty() {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(label).size(12.0));
    }
    toggled
}

fn action_btn(ui: &mut egui::Ui, text: &str, color: egui::Color32) -> bool {
    let prev = ui.spacing().button_padding;
    ui.spacing_mut().button_padding = egui::vec2(16.0, prev.y);
    let clicked = ui.add(egui::Button::new(
        egui::RichText::new(text).size(14.0).strong().color(egui::Color32::WHITE)
    ).fill(color)
     .rounding(8.0)
     .min_size(egui::vec2(0.0, 38.0))
    ).clicked();
    ui.spacing_mut().button_padding = prev;
    clicked
}

fn field_label(ui: &mut egui::Ui, text: &str, th: &Th) {
    ui.label(egui::RichText::new(text).size(12.0).strong().color(th.text));
}

fn text_field(ui: &mut egui::Ui, buf: &mut String, hint: &str, th: &Th, pw: bool) -> egui::Response {
    let mut te = egui::TextEdit::singleline(buf)
        .desired_width(f32::INFINITY)
        .margin(egui::Margin::symmetric(12.0, 10.0))
        .hint_text(egui::RichText::new(hint).color(th.muted));
    if pw { te = te.password(true); }

    let mut resp = None;
    egui::Frame::none()
        .fill(th.input)
        .rounding(8.0)
        .stroke(egui::Stroke::new(1.0, th.border))
        .show(ui, |ui: &mut egui::Ui| {
            resp = Some(ui.add(te));
        });
    resp.unwrap()
}

fn apply_visuals(ctx: &egui::Context, dark: bool) {
    let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.window_rounding = 10.0.into();
    v.widgets.noninteractive.rounding = 6.0.into();
    v.widgets.inactive.rounding = 6.0.into();
    v.widgets.hovered.rounding = 6.0.into();
    v.widgets.active.rounding = 6.0.into();
    if dark {
        v.panel_fill = D_BG;
        v.extreme_bg_color = egui::Color32::from_rgb(14, 14, 18);
        v.widgets.inactive.bg_fill = D_INPUT;
        v.widgets.inactive.weak_bg_fill = D_INPUT;
    } else {
        v.panel_fill = L_BG;
        v.extreme_bg_color = egui::Color32::from_rgb(250, 250, 254);
        v.widgets.inactive.bg_fill = L_INPUT;
        v.widgets.inactive.weak_bg_fill = L_INPUT;
    }
    ctx.set_visuals(v);
}

// ═══ Service management ═══
fn svc_file_path() -> Option<std::path::PathBuf> {
    dirs::home_dir().map(|h| h.join(".config/systemd/user/noip-duc.service"))
}

fn svc_env_path() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|mut p| {
        p.push("noip-duc");
        let _ = fs::create_dir_all(&p);
        p.push("service.env");
        p
    })
}

fn find_duc_binary() -> Option<String> {
    // Check same directory as current executable
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("noip-duc");
            if candidate.exists() {
                return candidate.to_str().map(|s| s.to_string());
            }
        }
    }
    // Check PATH
    if let Ok(out) = std::process::Command::new("which").arg("noip-duc").output() {
        if out.status.success() {
            let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !p.is_empty() { return Some(p); }
        }
    }
    None
}

fn svc_install(username: &str, password: &str, hostnames: &str) -> Result<(), String> {
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

    std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .output()
        .map_err(|e| format!("Failed to reload systemd: {e}"))?;
    Ok(())
}

fn svc_uninstall() -> Result<(), String> {
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "stop", "noip-duc.service"]).output();
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "disable", "noip-duc.service"]).output();
    if let Some(p) = svc_file_path() { let _ = fs::remove_file(p); }
    if let Some(p) = svc_env_path() { let _ = fs::remove_file(p); }
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"]).output();
    Ok(())
}

fn svc_start() -> Result<(), String> {
    let out = std::process::Command::new("systemctl")
        .args(["--user", "start", "noip-duc.service"])
        .output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(())
}

fn svc_stop() -> Result<(), String> {
    let out = std::process::Command::new("systemctl")
        .args(["--user", "stop", "noip-duc.service"])
        .output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(())
}

fn svc_enable() -> Result<(), String> {
    let out = std::process::Command::new("systemctl")
        .args(["--user", "enable", "noip-duc.service"])
        .output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(())
}

fn svc_disable() -> Result<(), String> {
    let out = std::process::Command::new("systemctl")
        .args(["--user", "disable", "noip-duc.service"])
        .output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(())
}

// ═══ Observer bridge ═══
#[derive(Clone)]
struct ChannelObs(mpsc::Sender<Notification>);
impl noip_duc::Observer for ChannelObs {
    fn notify(&self, n: Notification) { let _ = self.0.send(n); }
}

// ═══ Entry ═══
fn main() -> eframe::Result<()> {
    env_logger::init();
    let cfg = AppConfig::load();

    let icon = {
        let img = image::load_from_memory(LOGO_PNG).expect("icon decode");
        let rgba = img.to_rgba8();
        egui::IconData {
            width: rgba.width(),
            height: rgba.height(),
            rgba: rgba.into_raw(),
        }
    };

    eframe::run_native("No-IP DUC", eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([540.0, 650.0])
            .with_min_inner_size([420.0, 480.0])
            .with_app_id("com.noip.DUC")
            .with_icon(Arc::new(icon)),
        ..Default::default()
    }, Box::new(|cc| Ok(Box::new(DucApp::new(cc, cfg)))))
}
