use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use eframe::egui;

use noip_duc::{updater, Control, Notification};

use crate::config::AppConfig;
use crate::service;
use crate::tasks::{self, ChannelObs};
use crate::theme::{self, Th, AMBER, BLUE, GREEN, RED};
use crate::widgets::*;

const LOGO_PNG: &[u8] = include_bytes!("../../../logo.png");
const LOGO_SM_PNG: &[u8] = include_bytes!("../../../logo_64.png");

#[derive(PartialEq)]
enum Screen {
    Login,
    Fetching,
    Dashboard,
}

pub enum FetchResult {
    Ok(Vec<String>),
    Err(String),
}

struct LogEntry {
    ts: String,
    msg: String,
    level: u8,
}

/// Return current local time as HH:MM:SS.
fn now_hms() -> String {
    // Compute local time using libc to respect the system timezone.
    #[cfg(unix)]
    {
        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        // SAFETY: localtime_r writes into the provided buffer and does not
        // retain any pointers to it. The `time` pointer is valid for the call.
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        unsafe { libc::localtime_r(&epoch, &mut tm) };
        return format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec);
    }
    #[cfg(not(unix))]
    {
        let s = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        format!("{:02}:{:02}:{:02}", (s % 86400) / 3600, (s % 3600) / 60, s % 60)
    }
}

/// Load embedded PNG bytes into an egui texture. Returns `None` on decode failure
/// instead of panicking.
fn load_tex(ctx: &egui::Context, name: &str, bytes: &[u8]) -> Option<egui::TextureHandle> {
    let img = image::load_from_memory(bytes).ok()?;
    let rgba = img.to_rgba8();
    let sz = [rgba.width() as usize, rgba.height() as usize];
    let ci = egui::ColorImage::from_rgba_unmultiplied(sz, &rgba.into_raw());
    Some(ctx.load_texture(name, ci, egui::TextureOptions::LINEAR))
}

pub struct DucApp {
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
    logo: Option<egui::TextureHandle>,
    logo_sm: Option<egui::TextureHandle>,
    svc_installed: bool,
    svc_running: bool,
    svc_enabled: bool,
    svc_msg: String,
}

impl DucApp {
    pub fn new(cc: &eframe::CreationContext<'_>, cfg: AppConfig) -> Self {
        theme::apply_visuals(&cc.egui_ctx, cfg.dark_mode);
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
            if !app.cfg.selected_hosts().is_empty() {
                app.start_updater();
            }
        }
        app.refresh_svc();
        app
    }

    fn kick_ip(&mut self, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        self.ip_rx = Some(rx);
        tasks::detect_ip(tx, ctx.clone());
    }

    fn do_login(&mut self, ctx: &egui::Context) {
        self.cfg.username = self.cfg.username.trim().to_string();
        self.cfg.password = self.pw_input.clone();
        self.cfg.api_key = self.api_key_input.clone();

        if self.save_pw_checked {
            self.cfg.save_password = true;
        } else {
            self.cfg.save_password = false;
            self.cfg.password = String::new();
        }
        self.cfg.save();
        // Restore runtime password regardless of persistence choice.
        self.cfg.password = self.pw_input.clone();

        if !self.api_key_input.is_empty() {
            let key = self.api_key_input.clone();
            let ctx2 = ctx.clone();
            let (tx, rx) = mpsc::channel();
            self.fetch_rx = Some(rx);
            self.screen = Screen::Fetching;
            thread::spawn(move || {
                let res = match tasks::fetch_hosts_api(&key) {
                    Ok(hosts) => FetchResult::Ok(hosts),
                    Err(e) => FetchResult::Err(e),
                };
                let _ = tx.send(res);
                ctx2.request_repaint();
            });
        } else {
            self.screen = Screen::Dashboard;
        }

        self.kick_ip(ctx);
    }

    fn start_updater(&mut self) {
        let sel = self.cfg.selected_hosts();
        if sel.is_empty() { return; }
        if let Some(tx) = self.ctrl_tx.take() { let _ = tx.send(Control::Quit); }

        let (ntx, nrx) = mpsc::channel();
        let (ctx, crx) = mpsc::channel();
        self.notif_rx = Some(nrx);
        self.ctrl_tx = Some(ctx);
        let user = self.cfg.username.clone();
        let pass = self.cfg.password.clone();
        let n = sel.len();

        thread::spawn(move || {
            let im: noip_duc::public_ip::IpMethods = "dns,http,http-port-8245".parse().unwrap_or_default();
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
        self.logs.push(LogEntry { ts: now_hms(), msg: msg.to_string(), level });
    }

    fn drain(&mut self) {
        if let Some(rx) = &self.ip_rx {
            if let Ok(ip) = rx.try_recv() {
                self.ip = ip;
            }
        }
        if let Some(rx) = &self.notif_rx {
            // Collect into Vec to release the immutable borrow on self before logging.
            let msgs: Vec<_> = rx.try_iter().collect();
            for n in msgs {
                match n {
                    Notification::CheckIp => self.status_text = "Checking IP\u{2026}".into(),
                    Notification::IpChanged { current, .. } => {
                        self.ip = current.to_string();
                        self.log(0, &format!("IP changed to {current}"));
                    }
                    Notification::Updated { current, .. } => {
                        self.log(0, &format!("DNS updated \u{2192} {current}"));
                    }
                    Notification::NoUpdateNeeded(ip) => {
                        self.status_text = format!("Up to date ({ip})");
                    }
                    Notification::NextCheck(d) => {
                        self.next_check = Some(humantime::format_duration(d).to_string());
                        self.status_text = "Running".into();
                    }
                    Notification::UpdateFailed(e) => {
                        self.log(2, &format!("Update failed: {e:?}"));
                    }
                    Notification::Error(e) => { self.log(2, &e); }
                    Notification::Quitting => {
                        self.running = false;
                        self.status_text = "Stopped".into();
                    }
                    _ => {}
                }
            }
        }
    }

    fn refresh_svc(&mut self) {
        let (installed, running, enabled) = service::svc_status();
        self.svc_installed = installed;
        self.svc_running = running;
        self.svc_enabled = enabled;
    }

    // ═══ Drawing ═══

    fn draw_login(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let th = Th::new(self.cfg.dark_mode);
        ui.add_space(20.0);
        ui.vertical_centered(|ui: &mut egui::Ui| {
            if let Some(ref logo) = self.logo {
                ui.image(egui::load::SizedTexture::new(logo.id(), egui::vec2(80.0, 80.0)));
            }
            ui.add_space(10.0);
            ui.label(egui::RichText::new("Sign in to No-IP").size(20.0).strong());
            ui.add_space(4.0);
            ui.label(egui::RichText::new("Enter your No-IP credentials").size(12.0).color(th.muted));
        });
        ui.add_space(20.0);

        card(ui, &th, |ui| {
            field_label(ui, "Username / Email", &th);
            ui.add_space(4.0);
            text_field(ui, &mut self.cfg.username, "you@example.com", &th, false);
            ui.add_space(12.0);

            field_label(ui, "Password", &th);
            ui.add_space(4.0);
            text_field(ui, &mut self.pw_input, "••••••••", &th, true);
            ui.add_space(8.0);

            ui.horizontal(|ui: &mut egui::Ui| {
                green_checkbox(ui, &mut self.save_pw_checked, "Remember password");
            });
            ui.add_space(12.0);

            // Optional API key toggle
            if ui.add(egui::Button::new(
                egui::RichText::new(if self.show_api_key { "v  Hide API key" } else { ">  I have an API key" }).size(12.0).color(th.muted)
            ).frame(false)).clicked() {
                self.show_api_key = !self.show_api_key;
            }

            if self.show_api_key {
                ui.add_space(8.0);
                field_label(ui, "API Key (auto-detect hosts)", &th);
                ui.add_space(4.0);
                text_field(ui, &mut self.api_key_input, "Bearer token", &th, false);
            }

            ui.add_space(16.0);

            if !self.login_err.is_empty() {
                ui.label(egui::RichText::new(&self.login_err).size(12.0).color(RED));
                ui.add_space(8.0);
            }

            let can_login = !self.cfg.username.trim().is_empty() && !self.pw_input.is_empty();
            ui.add_enabled_ui(can_login, |ui| {
                ui.vertical_centered(|ui: &mut egui::Ui| {
                    if action_btn(ui, "Sign In", GREEN) {
                        self.do_login(ctx);
                    }
                });
            });
        });
    }

    fn draw_fetching(&mut self, ui: &mut egui::Ui) {
        let th = Th::new(self.cfg.dark_mode);
        ui.add_space(60.0);
        ui.vertical_centered(|ui: &mut egui::Ui| {
            ui.spinner();
            ui.add_space(14.0);
            ui.label(egui::RichText::new("Fetching hostnames\u{2026}").size(15.0).color(th.muted));
        });
    }

    fn draw_dashboard(&mut self, ui: &mut egui::Ui) {
        let th = Th::new(self.cfg.dark_mode);

        egui::ScrollArea::vertical().show(ui, |ui: &mut egui::Ui| {
            ui.set_width(ui.available_width());

            // ════ STATUS CARD ════
            card(ui, &th, |ui: &mut egui::Ui| {
                ui.horizontal(|ui: &mut egui::Ui| {
                    if let Some(ref logo) = self.logo_sm {
                        ui.image(egui::load::SizedTexture::new(logo.id(), egui::vec2(38.0, 38.0)));
                        ui.add_space(8.0);
                    }
                    ui.vertical(|ui: &mut egui::Ui| {
                        ui.label(egui::RichText::new("DUC Status").size(16.0).strong());
                        ui.label(egui::RichText::new(&self.status_text).size(12.0).color(th.muted));
                    });

                    let (dot_col, label) = if self.running {
                        (GREEN, "Running")
                    } else {
                        (th.muted, "Stopped")
                    };

                    let (_, dot_rect) = ui.allocate_space(egui::vec2(10.0, 10.0));
                    ui.painter().circle_filled(dot_rect.center(), 5.0, dot_col);
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(label).size(15.0).strong().color(dot_col));

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui: &mut egui::Ui| {
                        if self.running {
                            if action_btn(ui, "\u{21BB} Refresh", GREEN) {
                                if let Some(tx) = &self.ctrl_tx {
                                    let _ = tx.send(Control::UpdateNow);
                                }
                            }
                            ui.add_space(6.0);
                            if action_btn(ui, "\u{25A0} Stop", RED) {
                                self.stop_updater();
                            }
                        } else if action_btn(ui, "\u{25B6} Start", GREEN) {
                            self.start_updater();
                        }
                    });
                });

                ui.add_space(12.0);

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
            card(ui, &th, |ui: &mut egui::Ui| {
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

                if let Some(h) = remove {
                    self.cfg.hosts.remove(&h);
                    changed = true;
                }

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
                    if self.running {
                        self.stop_updater();
                        self.start_updater();
                    }
                }
            });

            // ════ BACKGROUND SERVICE CARD ════
            card(ui, &th, |ui: &mut egui::Ui| {
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
                    } else if action_btn(ui, "Install Service", GREEN) {
                        let hostnames = hosts.join(",");
                        match service::svc_install(&self.cfg.username, &self.cfg.password, &hostnames) {
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
                } else {
                    ui.horizontal(|ui: &mut egui::Ui| {
                        if self.svc_running {
                            if action_btn(ui, "Stop", RED) {
                                match service::svc_stop() {
                                    Ok(()) => self.svc_msg.clear(),
                                    Err(e) => self.svc_msg = e,
                                }
                                self.refresh_svc();
                            }
                        } else if action_btn(ui, "Start", GREEN) {
                            match service::svc_start() {
                                Ok(()) => self.svc_msg.clear(),
                                Err(e) => self.svc_msg = e,
                            }
                            self.refresh_svc();
                        }
                        ui.add_space(6.0);
                        if self.svc_enabled {
                            if action_btn(ui, "Disable Autostart", AMBER) {
                                let _ = service::svc_disable();
                                self.refresh_svc();
                            }
                        } else if action_btn(ui, "Enable Autostart", BLUE) {
                            let _ = service::svc_enable();
                            self.refresh_svc();
                        }
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui: &mut egui::Ui| {
                        if action_btn(ui, "Update Config", BLUE) {
                            let hosts = self.cfg.selected_hosts().join(",");
                            match service::svc_install(&self.cfg.username, &self.cfg.password, &hosts) {
                                Ok(()) => {
                                    if self.svc_running {
                                        let _ = service::svc_stop();
                                        let _ = service::svc_start();
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
                            let _ = service::svc_uninstall();
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
            card(ui, &th, |ui: &mut egui::Ui| {
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
    }
}

impl eframe::App for DucApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain();

        // Handle fetch result
        if let Some(rx) = &self.fetch_rx {
            if let Ok(res) = rx.try_recv() {
                match res {
                    FetchResult::Ok(hosts) => {
                        for h in hosts {
                            self.cfg.hosts.entry(h).or_insert(true);
                        }
                        self.cfg.save();
                        self.screen = Screen::Dashboard;
                        if !self.cfg.selected_hosts().is_empty() {
                            self.start_updater();
                        }
                    }
                    FetchResult::Err(e) => {
                        self.login_err = e;
                        self.screen = Screen::Login;
                    }
                }
                self.fetch_rx = None;
            }
        }

        if self.running {
            ctx.request_repaint_after(Duration::from_secs(1));
        }

        let th = Th::new(self.cfg.dark_mode);

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(th.bg).inner_margin(egui::Margin::symmetric(24.0, 16.0)))
            .show(ctx, |ui: &mut egui::Ui| {
                // Top bar
                ui.horizontal(|ui: &mut egui::Ui| {
                    if let Some(ref logo) = self.logo_sm {
                        ui.image(egui::load::SizedTexture::new(logo.id(), egui::vec2(28.0, 28.0)));
                        ui.add_space(6.0);
                    }
                    ui.label(egui::RichText::new("No-IP DUC").size(17.0).strong());

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui: &mut egui::Ui| {
                        if self.screen == Screen::Dashboard {
                            ui.add_space(12.0);
                            if ui.add(
                                egui::Button::new(
                                    egui::RichText::new("Sign Out").size(13.0).strong().color(egui::Color32::WHITE),
                                )
                                .fill(RED)
                                .rounding(8.0)
                                .min_size(egui::vec2(0.0, 30.0)),
                            ).clicked() {
                                self.stop_updater();
                                self.cfg.username.clear();
                                self.cfg.password.clear();
                                self.pw_input.clear();
                                self.cfg.save_password = false;
                                self.save_pw_checked = false;
                                self.cfg.save();
                                self.screen = Screen::Login;
                            }
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new(&self.cfg.username).size(12.0).color(th.muted));
                        }

                        let icon = if self.cfg.dark_mode { "\u{2600}" } else { "\u{1F319}" };
                        if theme_btn(ui, icon, &th) {
                            self.cfg.dark_mode = !self.cfg.dark_mode;
                            theme::apply_visuals(ctx, self.cfg.dark_mode);
                            self.cfg.save();
                        }
                    });
                });

                ui.add_space(12.0);

                match self.screen {
                    Screen::Login => self.draw_login(ui, ctx),
                    Screen::Fetching => self.draw_fetching(ui),
                    Screen::Dashboard => self.draw_dashboard(ui),
                }
            });
    }
}
