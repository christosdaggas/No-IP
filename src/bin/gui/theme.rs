use eframe::egui;
use eframe::egui::Color32;

/// Max width of the centered card column. Sized to match the Lovable mockup
/// (`max-w-[640px]` minus our outer horizontal padding).
pub const CARD_COL_WIDTH: f32 = 620.0;

// ═══ Brand palette ═══
pub const GREEN: Color32 = Color32::from_rgb(58, 203, 111);
pub const RED:   Color32 = Color32::from_rgb(238, 70, 70);
pub const BLUE:  Color32 = Color32::from_rgb(56, 130, 216);
pub const AMBER: Color32 = Color32::from_rgb(230, 168, 28);

/// Soft fill behind the logo badge and the activity-log check icon.
/// Premultiplied form of `(58, 203, 111)` at α=46/255 (≈18%).
pub const PALE_GREEN: Color32 = Color32::from_rgba_premultiplied(10, 36, 20, 46);

// Light surface
const L_BG:      Color32 = Color32::from_rgb(0xF4, 0xF5, 0xF7);
const L_CARD:    Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
const L_BORDER:  Color32 = Color32::from_rgb(0xDA, 0xDC, 0xE2);
const L_INPUT:   Color32 = Color32::from_rgb(0xF1, 0xF2, 0xF6);
const L_TEXT:    Color32 = Color32::from_rgb(0x1C, 0x1E, 0x26);
const L_MUTED:   Color32 = Color32::from_rgb(0x76, 0x7A, 0x86);
const L_ROW_ALT: Color32 = Color32::from_rgb(0xF6, 0xF7, 0xFA);

// Dark surface
const D_BG:      Color32 = Color32::from_rgb(0x0E, 0x10, 0x15);
const D_CARD:    Color32 = Color32::from_rgb(0x1C, 0x1E, 0x25);
const D_BORDER:  Color32 = Color32::from_rgb(0x3A, 0x3C, 0x44);
const D_INPUT:   Color32 = Color32::from_rgb(0x2A, 0x2C, 0x34);
const D_TEXT:    Color32 = Color32::from_rgb(0xF0, 0xF1, 0xF4);
const D_MUTED:   Color32 = Color32::from_rgb(0xA0, 0xA3, 0xAA);
const D_ROW_ALT: Color32 = Color32::from_rgb(0x21, 0x23, 0x2B);

#[derive(Clone, Copy)]
pub struct Th {
    pub bg: Color32,
    pub card: Color32,
    pub border: Color32,
    pub input: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub row_alt: Color32,
}

impl Th {
    pub fn new(dark: bool) -> Self {
        if dark {
            Self { bg: D_BG, card: D_CARD, border: D_BORDER, input: D_INPUT,
                   text: D_TEXT, muted: D_MUTED, row_alt: D_ROW_ALT }
        } else {
            Self { bg: L_BG, card: L_CARD, border: L_BORDER, input: L_INPUT,
                   text: L_TEXT, muted: L_MUTED, row_alt: L_ROW_ALT }
        }
    }
}

pub fn apply_visuals(ctx: &egui::Context, dark: bool) {
    let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    let th = Th::new(dark);

    v.window_rounding = 16.0.into();
    v.widgets.noninteractive.rounding = 10.0.into();
    v.widgets.inactive.rounding = 10.0.into();
    v.widgets.hovered.rounding = 10.0.into();
    v.widgets.active.rounding = 10.0.into();

    v.panel_fill = th.bg;
    v.extreme_bg_color = th.input;
    v.widgets.inactive.bg_fill = th.input;
    v.widgets.inactive.weak_bg_fill = th.input;
    v.widgets.noninteractive.fg_stroke.color = th.text;
    v.widgets.inactive.fg_stroke.color = th.text;
    v.widgets.hovered.fg_stroke.color = th.text;
    v.widgets.active.fg_stroke.color = th.text;
    v.widgets.noninteractive.bg_stroke.color = th.border;

    ctx.set_visuals(v);
}
