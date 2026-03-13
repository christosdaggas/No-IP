use eframe::egui;

// ═══ Brand palette ═══
pub const GREEN: egui::Color32 = egui::Color32::from_rgb(141, 189, 9);
pub const RED: egui::Color32 = egui::Color32::from_rgb(215, 58, 58);
pub const BLUE: egui::Color32 = egui::Color32::from_rgb(56, 130, 216);
pub const AMBER: egui::Color32 = egui::Color32::from_rgb(230, 168, 28);

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

/// Resolved theme colors for the current mode.
#[derive(Clone, Copy)]
pub struct Th {
    pub bg: egui::Color32,
    pub card: egui::Color32,
    pub border: egui::Color32,
    pub input: egui::Color32,
    pub text: egui::Color32,
    pub muted: egui::Color32,
    pub row_alt: egui::Color32,
}

impl Th {
    pub fn new(dark: bool) -> Self {
        if dark {
            Self { bg: D_BG, card: D_CARD, border: D_BORDER, input: D_INPUT, text: D_TEXT, muted: D_MUTED, row_alt: D_ROW_ALT }
        } else {
            Self { bg: L_BG, card: L_CARD, border: L_BORDER, input: L_INPUT, text: L_TEXT, muted: L_MUTED, row_alt: L_ROW_ALT }
        }
    }
}

pub fn apply_visuals(ctx: &egui::Context, dark: bool) {
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
