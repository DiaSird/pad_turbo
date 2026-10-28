//! The start/stop toggle section.

use super::{GREEN, RED};
use eframe::egui::{self, Color32, RichText};
use engine::Shared;
use i18n::Tr;
use input::binding::{Binding, DEFAULT_TOGGLE};

/// Shows and edits the start/stop toggle binding.
///
/// Three ways to change it: record it by pressing the inputs, type it into
/// the text field (syntax in [`input::binding`]), or reset it to
/// [`DEFAULT_TOGGLE`]. Editing is disabled while a function runs so that the
/// binding cannot change under the user's fingers.
pub(super) fn show(
    ui: &mut egui::Ui,
    sh: &mut Shared,
    tr: Tr,
    text: &mut String,
    error: &mut Option<String>,
) {
    ui.horizontal(|ui| {
        ui.label(tr.t("toggle.current"));
        ui.label(
            RichText::new(format!(" {} ", sh.binding.label()))
                .monospace()
                .strong()
                .color(Color32::WHITE)
                .background_color(GREEN),
        );
    });

    ui.add_enabled_ui(!sh.is_running(), |ui| {
        ui.horizontal(|ui| {
            ui.toggle_value(&mut sh.binding_capture, tr.t("toggle.capture"));
            let reset = tr.fmt("toggle.reset", &[("key", DEFAULT_TOGGLE.to_string())]);
            if ui.button(reset).clicked() {
                sh.binding = Binding::default();
                sh.binding_capture = false;
                *error = None;
            }
        });
        if sh.binding_capture {
            ui.colored_label(GREEN, tr.t("toggle.capturing"));
        }

        ui.horizontal(|ui| {
            ui.label(tr.t("toggle.edit"));
            let field = ui.add(
                egui::TextEdit::singleline(text)
                    .desired_width(200.0)
                    .hint_text("Ctrl+F5"),
            );
            let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button(tr.t("toggle.apply")).clicked() || entered {
                match Binding::parse(text) {
                    Ok(b) => {
                        sh.binding = b;
                        *error = None;
                    }
                    Err(token) => *error = Some(token),
                }
            }
        });
        if let Some(token) = error {
            ui.colored_label(RED, tr.fmt("toggle.invalid", &[("token", token.clone())]));
        }
    });
    ui.label(RichText::new(tr.t("toggle.help")).small().weak());
}
