use eframe::egui;
use eframe::egui::Color32;

use crate::theme::{Th, GREEN, PALE_GREEN};

const CARD_PAD_X: f32 = 20.0;
const CARD_PAD_Y: f32 = 18.0;

/// Width-locked rounded card. The inner ui width is pinned via
/// `set_min_width = set_max_width = width - 2*pad`, so sibling cards in a
/// column always end at the same x-coordinate regardless of inner content.
pub fn card(ui: &mut egui::Ui, th: &Th, width: f32, content: impl FnOnce(&mut egui::Ui)) {
    let inner = (width - 2.0 * CARD_PAD_X).max(120.0);
    egui::Frame::none()
        .fill(th.card)
        .rounding(16.0)
        .inner_margin(egui::Margin::symmetric(CARD_PAD_X, CARD_PAD_Y))
        .stroke(egui::Stroke::new(1.0, th.border))
        .show(ui, |ui| {
            ui.set_min_width(inner);
            ui.set_max_width(inner);
            content(ui);
        });
}

/// Pale-green circular badge with the No-IP logo centered inside.
pub fn logo_badge(ui: &mut egui::Ui, logo: Option<&egui::TextureHandle>, size: f32) {
    let (_, rect) = ui.allocate_space(egui::vec2(size, size));
    ui.painter().circle_filled(rect.center(), size / 2.0, PALE_GREEN);
    if let Some(tex) = logo {
        let inner = size * 0.6;
        let r = egui::Rect::from_center_size(rect.center(), egui::vec2(inner, inner));
        ui.painter().image(
            tex.id(),
            r,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }
}

/// Small green check inside a pale-green circle — used for info-level activity entries.
pub fn green_check(ui: &mut egui::Ui, size: f32) {
    let (_, rect) = ui.allocate_space(egui::vec2(size, size));
    ui.painter().circle_filled(rect.center(), size / 2.0, PALE_GREEN);
    let s = egui::Stroke::new(1.8, GREEN);
    let p0 = egui::pos2(rect.left() + size * 0.28, rect.center().y + size * 0.04);
    let p1 = egui::pos2(rect.left() + size * 0.46, rect.bottom() - size * 0.26);
    let p2 = egui::pos2(rect.right() - size * 0.24, rect.top() + size * 0.30);
    ui.painter().line_segment([p0, p1], s);
    ui.painter().line_segment([p1, p2], s);
}

/// Small filled dot inside a soft halo of the same hue. Used for warn/error
/// activity entries — avoids font-glyph fallback issues.
pub fn status_dot_chip(ui: &mut egui::Ui, size: f32, color: Color32) {
    let (_, rect) = ui.allocate_space(egui::vec2(size, size));
    let halo = Color32::from_rgba_premultiplied(
        (color.r() as u32 * 46 / 255) as u8,
        (color.g() as u32 * 46 / 255) as u8,
        (color.b() as u32 * 46 / 255) as u8,
        46,
    );
    ui.painter().circle_filled(rect.center(), size / 2.0, halo);
    ui.painter().circle_filled(rect.center(), size * 0.22, color);
}

/// Round chip-shaped icon button — 28×28, muted icon on input-tint fill.
pub fn chip_button(ui: &mut egui::Ui, glyph: &str, th: &Th) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(glyph).size(14.0).color(th.muted))
            .fill(th.input)
            .stroke(egui::Stroke::NONE)
            .rounding(99.0)
            .min_size(egui::vec2(28.0, 28.0)),
    )
}

/// Custom-painted theme toggle (sun in dark mode, moon in light mode).
/// Avoids font-glyph fallback issues — every shape is drawn with the painter.
pub fn theme_chip(ui: &mut egui::Ui, dark_mode: bool, th: &Th) -> egui::Response {
    let size = 28.0;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
    let hovered = resp.hovered();
    let p = ui.painter();
    let bg = if hovered { th.border } else { th.input };
    p.circle_filled(rect.center(), size / 2.0, bg);

    let cx = rect.center().x;
    let cy = rect.center().y;
    let icon = th.muted;

    if dark_mode {
        // Sun: small filled disc + 8 radial rays
        p.circle_filled(rect.center(), 3.5, icon);
        let r0 = 5.8;
        let r1 = 9.0;
        let stroke = egui::Stroke::new(1.5, icon);
        for i in 0..8 {
            let a = i as f32 * std::f32::consts::FRAC_PI_4;
            let s = egui::pos2(cx + a.cos() * r0, cy + a.sin() * r0);
            let e = egui::pos2(cx + a.cos() * r1, cy + a.sin() * r1);
            p.line_segment([s, e], stroke);
        }
    } else {
        // Crescent moon: filled disc with an offset bg-colour disc cut out.
        let r = 7.5;
        p.circle_filled(egui::pos2(cx - 1.0, cy), r, icon);
        p.circle_filled(egui::pos2(cx + 2.5, cy - 1.0), r, bg);
    }
    resp
}

/// Full-width pill button: 40px tall, white bold text. Width is enforced.
pub fn pill_button(ui: &mut egui::Ui, text: &str, color: Color32, width: f32) -> bool {
    let prev = ui.spacing().button_padding;
    ui.spacing_mut().button_padding = egui::vec2(10.0, prev.y);
    let resp = ui.add(
        egui::Button::new(
            egui::RichText::new(text).size(14.0).strong().color(Color32::WHITE),
        )
        .fill(color)
        .rounding(12.0)
        .min_size(egui::vec2(width, 40.0)),
    );
    ui.spacing_mut().button_padding = prev;
    resp.clicked()
}

/// Content-sized pill button — used in card headers where width is dynamic.
pub fn inline_button(ui: &mut egui::Ui, text: &str, color: Color32) -> bool {
    let prev = ui.spacing().button_padding;
    ui.spacing_mut().button_padding = egui::vec2(16.0, prev.y);
    let resp = ui.add(
        egui::Button::new(
            egui::RichText::new(text).size(14.0).strong().color(Color32::WHITE),
        )
        .fill(color)
        .rounding(12.0)
        .min_size(egui::vec2(0.0, 36.0)),
    );
    ui.spacing_mut().button_padding = prev;
    resp.clicked()
}

/// Rounded text input with optional password masking.
pub fn text_input(
    ui: &mut egui::Ui,
    buf: &mut String,
    hint: &str,
    th: &Th,
    password: bool,
) -> egui::Response {
    let mut te = egui::TextEdit::singleline(buf)
        .desired_width(f32::INFINITY)
        .margin(egui::Margin::symmetric(12.0, 10.0))
        .hint_text(egui::RichText::new(hint).color(th.muted));
    if password {
        te = te.password(true);
    }
    let mut resp = None;
    egui::Frame::none()
        .fill(th.input)
        .rounding(10.0)
        .stroke(egui::Stroke::new(1.0, th.border))
        .show(ui, |ui| {
            resp = Some(ui.add(te));
        });
    resp.unwrap()
}

/// Green-fill checkbox with a hand-drawn check mark.
pub fn green_checkbox(ui: &mut egui::Ui, checked: &mut bool, label: &str, th: &Th) -> bool {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
    let toggled = resp.clicked();
    if toggled {
        *checked = !*checked;
    }
    let visuals = ui.style().interact(&resp);
    if *checked {
        ui.painter().rect_filled(rect, 4.0, GREEN);
        let s = egui::Stroke::new(2.0, Color32::WHITE);
        let p0 = egui::pos2(rect.left() + 4.0, rect.center().y + 0.5);
        let p1 = egui::pos2(rect.left() + 7.5, rect.bottom() - 4.0);
        let p2 = egui::pos2(rect.right() - 3.5, rect.top() + 4.5);
        ui.painter().line_segment([p0, p1], s);
        ui.painter().line_segment([p1, p2], s);
    } else {
        ui.painter()
            .rect_stroke(rect, 4.0, egui::Stroke::new(1.5, visuals.fg_stroke.color));
    }
    if !label.is_empty() {
        ui.add_space(6.0);
        ui.label(egui::RichText::new(label).size(12.0).color(th.text));
    }
    toggled
}

/// Label-left, value-right info row for the DUC Status card.
pub fn info_row(ui: &mut egui::Ui, th: &Th, label: &str, value: egui::RichText) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).size(13.0).color(th.muted));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(value);
        });
    });
}

pub fn field_label(ui: &mut egui::Ui, text: &str, th: &Th) {
    ui.label(egui::RichText::new(text).size(12.0).strong().color(th.text));
}
