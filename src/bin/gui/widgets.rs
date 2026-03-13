use eframe::egui;

use crate::theme::{Th, GREEN};

/// A rounded card container with theme-aware fill and border.
pub fn card(ui: &mut egui::Ui, th: &Th, content: impl FnOnce(&mut egui::Ui)) {
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

/// Circular theme-toggle button.
pub fn theme_btn(ui: &mut egui::Ui, icon: &str, th: &Th) -> bool {
    ui.add(
        egui::Button::new(egui::RichText::new(icon).size(14.0))
            .fill(egui::Color32::TRANSPARENT)
            .stroke(egui::Stroke::new(1.0, th.border))
            .rounding(99.0)
            .min_size(egui::vec2(30.0, 30.0)),
    )
    .clicked()
}

/// Custom green checkbox with a hand-drawn checkmark.
pub fn green_checkbox(ui: &mut egui::Ui, checked: &mut bool, label: &str) -> bool {
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
        let stroke = egui::Stroke::new(2.0, egui::Color32::BLACK);
        let x0 = rect.left() + 3.5;
        let y0 = rect.center().y;
        let x1 = rect.left() + 6.5;
        let y1 = rect.bottom() - 3.5;
        let x2 = rect.right() - 3.0;
        let y2 = rect.top() + 4.0;
        ui.painter()
            .line_segment([egui::pos2(x0, y0), egui::pos2(x1, y1)], stroke);
        ui.painter()
            .line_segment([egui::pos2(x1, y1), egui::pos2(x2, y2)], stroke);
    } else {
        ui.painter().rect_stroke(
            rect,
            rounding,
            egui::Stroke::new(1.5, visuals.fg_stroke.color),
        );
    }
    if !label.is_empty() {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(label).size(12.0));
    }
    toggled
}

/// Colored action button with white bold text.
pub fn action_btn(ui: &mut egui::Ui, text: &str, color: egui::Color32) -> bool {
    let prev = ui.spacing().button_padding;
    ui.spacing_mut().button_padding = egui::vec2(16.0, prev.y);
    let clicked = ui
        .add(
            egui::Button::new(
                egui::RichText::new(text)
                    .size(14.0)
                    .strong()
                    .color(egui::Color32::WHITE),
            )
            .fill(color)
            .rounding(8.0)
            .min_size(egui::vec2(0.0, 38.0)),
        )
        .clicked();
    ui.spacing_mut().button_padding = prev;
    clicked
}

/// Small bold label used above form fields.
pub fn field_label(ui: &mut egui::Ui, text: &str, th: &Th) {
    ui.label(egui::RichText::new(text).size(12.0).strong().color(th.text));
}

/// Themed single-line text field with optional password masking.
pub fn text_field(
    ui: &mut egui::Ui,
    buf: &mut String,
    hint: &str,
    th: &Th,
    pw: bool,
) -> egui::Response {
    let mut te = egui::TextEdit::singleline(buf)
        .desired_width(f32::INFINITY)
        .margin(egui::Margin::symmetric(12.0, 10.0))
        .hint_text(egui::RichText::new(hint).color(th.muted));
    if pw {
        te = te.password(true);
    }

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
