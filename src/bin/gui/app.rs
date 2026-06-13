use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use eframe::egui;

use noip_duc::{updater, Control, Notification};

use crate::config::AppConfig;
use crate::service;
use crate::tasks::{self, ChannelObs};
use crate::theme::{self, Th, AMBER, BLUE, CARD_COL_WIDTH, GREEN, RED};
use crate::widgets::*;

const LOGO_PNG: &[u8] = include_bytes!("../../../logo.png");
const LOGO_SM_PNG: &[u8] = include_bytes!("../../../logo_64.png");

#[derive(PartialEq, Clone, Copy)]
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

fn now_hms() -> String {
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
    svc_op_in_flight: Option<service::Op>,
    svc_op_rx: Option<mpsc::Receiver<service::OpResult>>,
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
            svc_op_in_flight: None,
            svc_op_rx: None,
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

    // ═══ Worker / state plumbing ═══

    fn kick_ip(&mut self, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        self.ip_rx = Some(rx);
        tasks::detect_ip(tx, ctx.clone());
    }

    fn do_login(&mut self, ctx: &egui::Context) {
        self.cfg.username = self.cfg.username.trim().to_string();
        self.cfg.password = self.pw_input.clone();
        self.cfg.api_key = self.api_key_input.clone();
        self.cfg.save_password = self.save_pw_checked;

        if self.save_pw_checked {
            self.cfg.remember_secrets();
        } else {
            self.cfg.forget_secrets();
        }
        self.cfg.save();

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

    fn do_sign_out(&mut self) {
        self.stop_updater();
        self.cfg.forget_secrets();
        self.cfg.username.clear();
        self.cfg.password.clear();
        self.cfg.api_key.clear();
        self.pw_input.clear();
        self.api_key_input.clear();
        self.cfg.save_password = false;
        self.save_pw_checked = false;
        self.cfg.save();
        self.screen = Screen::Login;
    }

    fn start_updater(&mut self) {
        let sel = self.cfg.selected_hosts();
        if sel.is_empty() {
            return;
        }
        if let Some(tx) = self.ctrl_tx.take() {
            let _ = tx.send(Control::Quit);
        }

        let (ntx, nrx) = mpsc::channel();
        let (ctx, crx) = mpsc::channel();
        self.notif_rx = Some(nrx);
        self.ctrl_tx = Some(ctx);
        let user = self.cfg.username.clone();
        let pass = self.cfg.password.clone();
        let n = sel.len();

        thread::spawn(move || {
            let im: noip_duc::public_ip::IpMethods =
                "dns,http,http-port-8245".parse().unwrap_or_default();
            let c = noip_duc::Config {
                username: &user,
                password: &pass,
                hostnames: Some(&sel),
                check_interval: Duration::from_secs(300),
                http_timeout: Duration::from_secs(10),
                exec_on_change: None,
                ip_method: &im,
                once: false,
            };
            let _ = updater(c, ChannelObs(ntx), crx);
        });
        self.running = true;
        self.status_text = "Running".into();
        self.log(0, &format!("Updater started \u{2014} {n} host(s)"));
    }

    fn stop_updater(&mut self) {
        if let Some(tx) = self.ctrl_tx.take() {
            let _ = tx.send(Control::Quit);
        }
        self.notif_rx = None;
        self.running = false;
        self.status_text = "Idle".into();
        self.next_check = None;
        self.log(0, "Updater stopped");
    }

    fn log(&mut self, level: u8, msg: &str) {
        self.logs.push(LogEntry {
            ts: now_hms(),
            msg: msg.to_string(),
            level,
        });
    }

    fn drain(&mut self) {
        if let Some(rx) = &self.svc_op_rx {
            if let Ok(res) = rx.try_recv() {
                let op_label = match res.op {
                    service::Op::Install => "Service installed",
                    service::Op::Uninstall => "Service removed",
                    service::Op::Start => "Service started",
                    service::Op::Stop => "Service stopped",
                    service::Op::Enable => "Autostart enabled",
                    service::Op::Disable => "Autostart disabled",
                    service::Op::UpdateConfig => "Service config updated",
                };
                match res.result {
                    Ok(()) => {
                        self.log(0, op_label);
                        self.svc_msg.clear();
                    }
                    Err(e) => {
                        self.log(2, &format!("{op_label} failed: {e}"));
                        self.svc_msg = e;
                    }
                }
                self.svc_op_in_flight = None;
                self.svc_op_rx = None;
                self.refresh_svc();
            }
        }

        if let Some(rx) = &self.ip_rx {
            if let Ok(ip) = rx.try_recv() {
                self.ip = ip;
            }
        }

        if let Some(rx) = &self.notif_rx {
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
                    Notification::Error(e) => {
                        self.log(2, &e);
                    }
                    Notification::Quitting => {
                        self.running = false;
                        self.status_text = "Idle".into();
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

    fn spawn_svc_op(&mut self, op: service::Op, args: service::OpArgs, ctx: &egui::Context) {
        if self.svc_op_in_flight.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.svc_op_rx = Some(rx);
        self.svc_op_in_flight = Some(op);
        self.svc_msg.clear();
        service::spawn_op(op, args, tx, ctx.clone());
    }

    // ═══ Drawing ═══

    fn draw_header(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, th: &Th) {
        ui.horizontal(|ui| {
            logo_badge(ui, self.logo_sm.as_ref(), 32.0);
            ui.add_space(10.0);
            ui.label(egui::RichText::new("No-IP DUC").size(18.0).strong().color(th.text));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut sign_out_clicked = false;
                if self.screen == Screen::Dashboard {
                    if inline_button(ui, "Sign Out", RED) {
                        sign_out_clicked = true;
                    }
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(&self.cfg.username)
                            .size(12.0)
                            .color(th.muted),
                    );
                    ui.add_space(8.0);
                }
                if theme_chip(ui, self.cfg.dark_mode, th)
                    .on_hover_text(if self.cfg.dark_mode {
                        "Switch to light mode"
                    } else {
                        "Switch to dark mode"
                    })
                    .clicked()
                {
                    self.cfg.dark_mode = !self.cfg.dark_mode;
                    theme::apply_visuals(ctx, self.cfg.dark_mode);
                    self.cfg.save();
                }
                if sign_out_clicked {
                    self.do_sign_out();
                }
            });
        });
    }

    fn draw_login(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, width: f32, th: &Th) {
        card(ui, th, width, |ui| {
            ui.vertical_centered(|ui| {
                logo_badge(ui, self.logo.as_ref(), 64.0);
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("Sign in to No-IP")
                        .size(20.0)
                        .strong()
                        .color(th.text),
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("Enter your No-IP credentials")
                        .size(12.0)
                        .color(th.muted),
                );
            });
            ui.add_space(18.0);

            field_label(ui, "Username / Email", th);
            ui.add_space(4.0);
            text_input(ui, &mut self.cfg.username, "you@example.com", th, false);
            ui.add_space(12.0);

            field_label(ui, "Password", th);
            ui.add_space(4.0);
            text_input(
                ui,
                &mut self.pw_input,
                "\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}",
                th,
                true,
            );
            ui.add_space(10.0);

            ui.horizontal(|ui| {
                green_checkbox(ui, &mut self.save_pw_checked, "Remember password", th);
            });
            ui.add_space(10.0);

            let toggle_text = if self.show_api_key {
                "Hide API key"
            } else {
                "I have an API key"
            };
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new(toggle_text).size(12.0).color(th.muted),
                    )
                    .frame(false),
                )
                .clicked()
            {
                self.show_api_key = !self.show_api_key;
            }

            if self.show_api_key {
                ui.add_space(8.0);
                field_label(ui, "API Key (auto-detect hosts)", th);
                ui.add_space(4.0);
                text_input(ui, &mut self.api_key_input, "Bearer token", th, false);
            }

            ui.add_space(16.0);

            if !self.login_err.is_empty() {
                ui.label(egui::RichText::new(&self.login_err).size(12.0).color(RED));
                ui.add_space(8.0);
            }

            let can_login =
                !self.cfg.username.trim().is_empty() && !self.pw_input.is_empty();
            // Render the button always-on so the white text stays legible;
            // dim the fill colour when the form is incomplete and gate the
            // click here instead of `add_enabled_ui` (which dims the text).
            let btn_color = if can_login { GREEN } else { GREEN.gamma_multiply(0.45) };
            let w = ui.available_width();
            if pill_button(ui, "Sign In", btn_color, w) && can_login {
                self.do_login(ctx);
            }
        });
    }

    fn draw_fetching(&self, ui: &mut egui::Ui, th: &Th) {
        ui.add_space(60.0);
        ui.vertical_centered(|ui| {
            ui.spinner();
            ui.add_space(14.0);
            ui.label(
                egui::RichText::new("Fetching hostnames\u{2026}")
                    .size(15.0)
                    .color(th.muted),
            );
        });
    }

    fn draw_status_card(&mut self, ui: &mut egui::Ui, width: f32, th: &Th) {
        let mut start_clicked = false;
        let mut stop_clicked = false;
        let mut refresh_clicked = false;

        card(ui, th, width, |ui| {
            ui.horizontal(|ui| {
                logo_badge(ui, self.logo_sm.as_ref(), 48.0);
                ui.add_space(12.0);

                let dot_color = if self.running { GREEN } else { th.muted };
                let status_word = if self.running { "Running" } else { "Stopped" };

                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("DUC Status")
                                .size(17.0)
                                .strong()
                                .color(th.text),
                        );
                        ui.add_space(8.0);
                        let (_, r) = ui.allocate_space(egui::vec2(8.0, 8.0));
                        ui.painter().circle_filled(r.center(), 4.0, dot_color);
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(status_word).size(13.0).color(th.muted),
                        );
                    });
                    ui.label(
                        egui::RichText::new(&self.status_text)
                            .size(12.0)
                            .color(th.muted),
                    );
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.running {
                        if inline_button(ui, "\u{25A0}  Stop", RED) {
                            stop_clicked = true;
                        }
                        ui.add_space(6.0);
                        if inline_button(ui, "Refresh", BLUE) {
                            refresh_clicked = true;
                        }
                    } else if inline_button(ui, "\u{25B6}  Start", GREEN) {
                        start_clicked = true;
                    }
                });
            });

            ui.add_space(14.0);

            info_row(
                ui,
                th,
                "IP Address",
                egui::RichText::new(&self.ip)
                    .size(13.0)
                    .strong()
                    .color(GREEN)
                    .monospace(),
            );
            ui.add_space(6.0);
            info_row(
                ui,
                th,
                "Account",
                egui::RichText::new(&self.cfg.username)
                    .size(13.0)
                    .color(th.text),
            );
            ui.add_space(6.0);
            let n = self.cfg.selected_hosts().len();
            let total = self.cfg.hosts.len();
            info_row(
                ui,
                th,
                "Hosts",
                egui::RichText::new(format!("{n} of {total} selected"))
                    .size(13.0)
                    .color(th.text),
            );
            if let Some(ref nc) = self.next_check {
                ui.add_space(6.0);
                info_row(
                    ui,
                    th,
                    "Next check",
                    egui::RichText::new(nc).size(13.0).color(th.muted),
                );
            }
        });

        if start_clicked {
            self.start_updater();
        }
        if stop_clicked {
            self.stop_updater();
        }
        if refresh_clicked {
            if let Some(tx) = &self.ctrl_tx {
                let _ = tx.send(Control::UpdateNow);
            }
        }
    }

    fn draw_hostnames_card(&mut self, ui: &mut egui::Ui, width: f32, th: &Th) {
        let mut changed = false;
        let mut to_remove: Option<String> = None;
        let mut add_clicked = false;

        card(ui, th, width, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Hostnames")
                        .size(15.0)
                        .strong()
                        .color(th.text),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new("select which to update")
                            .size(11.0)
                            .color(th.muted),
                    );
                });
            });
            ui.add_space(10.0);

            if self.cfg.hosts.is_empty() {
                ui.add_space(14.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("No hostnames yet")
                            .size(13.0)
                            .color(th.muted),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(
                            "Add one below, or sign in with an API key to auto-detect",
                        )
                        .size(11.0)
                        .color(th.muted),
                    );
                });
                ui.add_space(14.0);
            } else {
                let keys: Vec<String> = self.cfg.hosts.keys().cloned().collect();
                for h in &keys {
                    let checked = self.cfg.hosts.get_mut(h).unwrap();
                    egui::Frame::none()
                        .inner_margin(egui::Margin::symmetric(2.0, 6.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                if green_checkbox(ui, checked, "", th) {
                                    changed = true;
                                }
                                ui.add_space(10.0);
                                ui.label(
                                    egui::RichText::new(h)
                                        .size(13.0)
                                        .monospace()
                                        .color(th.text),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let resp = ui
                                            .add(
                                                egui::Button::new(
                                                    egui::RichText::new("\u{1F5D1}")
                                                        .size(14.0)
                                                        .color(th.muted),
                                                )
                                                .frame(false),
                                            )
                                            .on_hover_text("Remove");
                                        if resp.clicked() {
                                            to_remove = Some(h.clone());
                                        }
                                    },
                                );
                            });
                        });
                }
            }

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(10.0);

            // Add-host row: input fills, fixed button on the right.
            ui.horizontal(|ui| {
                let btn_w = 130.0;
                let sp = ui.spacing().item_spacing.x;
                let input_w = (ui.available_width() - btn_w - sp).max(120.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(input_w, 0.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_min_width(input_w);
                        ui.set_max_width(input_w);
                        text_input(ui, &mut self.host_input, "myhost.ddns.net", th, false);
                    },
                );
                if pill_button(ui, "+  Add Host", GREEN, btn_w) {
                    add_clicked = true;
                }
            });
        });

        if let Some(h) = to_remove {
            self.cfg.hosts.remove(&h);
            changed = true;
        }
        if add_clicked {
            let h = self.host_input.trim().to_string();
            if !h.is_empty() {
                self.cfg.hosts.entry(h).or_insert(true);
                self.host_input.clear();
                changed = true;
            }
        }
        if changed {
            self.cfg.save();
            if self.running {
                self.stop_updater();
                self.start_updater();
            }
        }
    }

    fn draw_service_card(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, width: f32, th: &Th) {
        let busy = self.svc_op_in_flight.is_some();
        // Capture intents from the closure and dispatch them after the card
        // closes so we never re-borrow self while painting.
        let mut intent: Option<(service::Op, service::OpArgs)> = None;
        let mut race_msg = false;

        card(ui, th, width, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new("Background Service")
                            .size(15.0)
                            .strong()
                            .color(th.text),
                    );
                    ui.add_space(2.0);
                    ui.label(
                        egui::RichText::new("Keep DNS updated even when the GUI is closed")
                            .size(11.0)
                            .color(th.muted),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (col, label) = if self.svc_running {
                        (GREEN, "Running")
                    } else if self.svc_installed {
                        (AMBER, "Stopped")
                    } else {
                        (th.muted, "Not installed")
                    };
                    ui.label(egui::RichText::new(label).size(12.0).color(col));
                    ui.add_space(6.0);
                    let (_, r) = ui.allocate_space(egui::vec2(8.0, 8.0));
                    ui.painter().circle_filled(r.center(), 4.0, col);
                });
            });
            ui.add_space(12.0);

            if let Some(op) = self.svc_op_in_flight {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(op.label()).size(12.0).color(th.muted));
                });
                ui.add_space(8.0);
            }

            ui.add_enabled_ui(!busy, |ui| {
                if !self.svc_installed {
                    let hosts = self.cfg.selected_hosts();
                    if hosts.is_empty() {
                        ui.label(
                            egui::RichText::new("Select at least one host before installing.")
                                .size(12.0)
                                .color(AMBER),
                        );
                    } else {
                        let w = ui.available_width();
                        if pill_button(ui, "Install Service", GREEN, w) {
                            intent = Some((
                                service::Op::Install,
                                service::OpArgs {
                                    username: self.cfg.username.clone(),
                                    password: self.cfg.password.clone(),
                                    hostnames: hosts.join(","),
                                },
                            ));
                        }
                    }
                } else {
                    let sp = ui.spacing().item_spacing.x;
                    let half = ((ui.available_width() - sp) / 2.0).max(100.0);

                    ui.horizontal(|ui| {
                        if self.svc_running {
                            if pill_button(ui, "\u{25A0}  Stop", RED, half) {
                                intent =
                                    Some((service::Op::Stop, service::OpArgs::default()));
                            }
                        } else if pill_button(ui, "\u{25B6}  Start", GREEN, half) {
                            if self.running {
                                race_msg = true;
                            } else {
                                intent =
                                    Some((service::Op::Start, service::OpArgs::default()));
                            }
                        }
                        if self.svc_enabled {
                            if pill_button(ui, "Disable Autostart", AMBER, half) {
                                intent = Some((
                                    service::Op::Disable,
                                    service::OpArgs::default(),
                                ));
                            }
                        } else if pill_button(ui, "Enable Autostart", BLUE, half) {
                            intent =
                                Some((service::Op::Enable, service::OpArgs::default()));
                        }
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if pill_button(ui, "Update Config", BLUE, half) {
                            intent = Some((
                                service::Op::UpdateConfig,
                                service::OpArgs {
                                    username: self.cfg.username.clone(),
                                    password: self.cfg.password.clone(),
                                    hostnames: self.cfg.selected_hosts().join(","),
                                },
                            ));
                        }
                        if pill_button(ui, "Uninstall", RED, half) {
                            intent = Some((
                                service::Op::Uninstall,
                                service::OpArgs::default(),
                            ));
                        }
                    });
                }
            });

            if self.running && self.svc_running {
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(
                        "Both the GUI updater and the background service are active. \
                         They'll send duplicate updates \u{2014} stop one.",
                    )
                    .size(11.0)
                    .color(AMBER),
                );
            }
            if !self.svc_msg.is_empty() {
                ui.add_space(8.0);
                ui.label(egui::RichText::new(&self.svc_msg).size(11.0).color(RED));
            }
        });

        if race_msg {
            self.svc_msg =
                "Stop the in-app updater before starting the background service.".into();
        }
        if let Some((op, args)) = intent {
            self.spawn_svc_op(op, args, ctx);
        }
    }

    fn draw_activity_card(&self, ui: &mut egui::Ui, width: f32, th: &Th) {
        card(ui, th, width, |ui| {
            ui.label(
                egui::RichText::new("Activity")
                    .size(15.0)
                    .strong()
                    .color(th.text),
            );
            ui.add_space(8.0);

            egui::ScrollArea::vertical()
                .id_source("actlog")
                .max_height(160.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    if self.logs.is_empty() {
                        ui.vertical_centered(|ui| {
                            ui.add_space(10.0);
                            ui.label(
                                egui::RichText::new("No activity yet")
                                    .size(12.0)
                                    .color(th.muted),
                            );
                        });
                    }
                    for e in &self.logs {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(&e.ts)
                                    .size(11.0)
                                    .color(th.muted)
                                    .monospace(),
                            );
                            ui.add_space(8.0);
                            match e.level {
                                0 => {
                                    green_check(ui, 16.0);
                                    ui.add_space(6.0);
                                    ui.label(
                                        egui::RichText::new(&e.msg)
                                            .size(12.0)
                                            .color(th.text),
                                    );
                                }
                                1 => {
                                    status_dot_chip(ui, 16.0, AMBER);
                                    ui.add_space(6.0);
                                    ui.label(
                                        egui::RichText::new(&e.msg)
                                            .size(12.0)
                                            .color(AMBER),
                                    );
                                }
                                _ => {
                                    status_dot_chip(ui, 16.0, RED);
                                    ui.add_space(6.0);
                                    ui.label(
                                        egui::RichText::new(&e.msg).size(12.0).color(RED),
                                    );
                                }
                            }
                        });
                    }
                });
        });
    }

    fn draw_dashboard(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, width: f32, th: &Th) {
        self.draw_status_card(ui, width, th);
        ui.add_space(14.0);
        self.draw_hostnames_card(ui, width, th);
        ui.add_space(14.0);
        self.draw_service_card(ui, ctx, width, th);
        ui.add_space(14.0);
        self.draw_activity_card(ui, width, th);
    }
}

impl eframe::App for DucApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain();

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
            .frame(egui::Frame::none().fill(th.bg))
            .show(ctx, |ui| {
                let panel_w = ui.available_width();
                let card_w = (panel_w - 32.0).min(CARD_COL_WIDTH).max(420.0);
                let pad = ((panel_w - card_w) / 2.0).max(16.0);

                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.add_space(pad);
                            ui.vertical(|ui| {
                                ui.set_min_width(card_w);
                                ui.set_max_width(card_w);
                                ui.add_space(16.0);
                                self.draw_header(ui, ctx, &th);
                                ui.add_space(14.0);
                                let screen = self.screen;
                                match screen {
                                    Screen::Login => self.draw_login(ui, ctx, card_w, &th),
                                    Screen::Fetching => self.draw_fetching(ui, &th),
                                    Screen::Dashboard => {
                                        self.draw_dashboard(ui, ctx, card_w, &th)
                                    }
                                }
                                ui.add_space(16.0);
                            });
                        });
                    });
            });
    }
}
