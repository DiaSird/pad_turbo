//! The output section: device choice, its status and the keyboard mapping.

use super::target_picker::{self, TargetPicker};
use super::{GREEN, RED};
use eframe::egui::{self, RichText};
use engine::{OutputPref, Shared};
use i18n::Tr;

/// Output device choice, its status, the keyboard target and the keyboard
/// mapping table.
pub(super) fn show(ui: &mut egui::Ui, sh: &mut Shared, tr: Tr, picker: &mut TargetPicker) {
    ui.add_enabled_ui(!sh.is_running(), |ui| {
        ui.horizontal(|ui| {
            ui.radio_value(
                &mut sh.settings.output,
                OutputPref::ViGEm,
                tr.t("output.vigem"),
            );
            ui.radio_value(
                &mut sh.settings.output,
                OutputPref::Keyboard,
                tr.t("output.keyboard"),
            );
        });
    });
    let ok = sh.output_is_vigem || sh.settings.output == OutputPref::Keyboard;
    ui.colored_label(if ok { GREEN } else { RED }, tr.msg(&sh.output_status));
    if sh.settings.output == OutputPref::ViGEm && !sh.output_is_vigem {
        ui.label(RichText::new(tr.t("output.driver_hint")).small());
    }
    if sh.output_is_vigem {
        ui.label(
            RichText::new(tr.t("output.passthrough_hint"))
                .small()
                .weak(),
        );
    } else {
        ui.add_space(4.0);
        target_picker::show(ui, sh, tr, picker);
        egui::CollapsingHeader::new(tr.t("output.keymap")).show(ui, |ui| {
            egui::Grid::new("keymap")
                .num_columns(4)
                .striped(true)
                .show(ui, |ui| {
                    for (i, (_, _, control, key)) in output::KEYMAP.iter().enumerate() {
                        ui.label(tr.get(&format!("control.{control}")).unwrap_or(control));
                        ui.label(RichText::new(*key).strong());
                        if i % 2 == 1 {
                            ui.end_row();
                        }
                    }
                });
        });
    }
}
