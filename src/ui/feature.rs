//! The feature section: Turbo / Record / Play tabs and their settings.

use super::{GREEN, secs};
use eframe::egui::{self, RichText};
use engine::{self, Feature, Shared, TurboMode};
use i18n::Tr;
use input::pad;

/// Interval presets offered next to the turbo interval field, in milliseconds.
const INTERVAL_PRESETS: [u32; 6] = [50, 100, 250, 500, 1000, 2000];

/// Feature tabs and the settings of the selected feature.
pub(super) fn show(ui: &mut egui::Ui, sh: &mut Shared, tr: Tr) {
    ui.add_enabled_ui(!sh.is_running(), |ui| {
        ui.horizontal(|ui| {
            for f in Feature::ALL {
                ui.selectable_value(
                    &mut sh.settings.feature,
                    f,
                    RichText::new(tr.t(f.key())).size(14.0),
                );
            }
        });
    });
    ui.separator();

    match sh.settings.feature {
        Feature::Turbo => turbo_settings(ui, sh, tr),
        Feature::Record => {
            ui.label(tr.t("record.description"));
            ui.label(tr.t("record.description2"));
            macro_info(ui, sh, tr);
        }
        Feature::Play => {
            ui.label(tr.t("play.description"));
            macro_info(ui, sh, tr);
        }
    }
}

/// Turbo buttons, interval and mode. Editable while turbo runs.
fn turbo_settings(ui: &mut egui::Ui, sh: &mut Shared, tr: Tr) {
    let s = &mut sh.settings;
    ui.label(tr.t("turbo.buttons"));
    egui::Grid::new("turbo_buttons")
        .num_columns(4)
        .show(ui, |ui| {
            for (i, (name, bit)) in pad::BUTTONS.iter().enumerate() {
                let mut on = s.turbo_mask & bit != 0;
                if ui.checkbox(&mut on, *name).changed() {
                    s.turbo_mask ^= bit;
                }
                if i % 4 == 3 {
                    ui.end_row();
                }
            }
        });
    ui.horizontal(|ui| {
        ui.toggle_value(&mut sh.capture, tr.t("turbo.capture"));
        if ui.button(tr.t("turbo.clear")).clicked() {
            s.turbo_mask = 0;
        }
    });
    if sh.capture {
        ui.colored_label(GREEN, tr.t("turbo.capture_hint"));
    }
    ui.label(tr.fmt(
        "turbo.selected",
        &[("buttons", pad::format_mask(s.turbo_mask))],
    ));

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(tr.t("turbo.interval"));
        ui.add(
            egui::DragValue::new(&mut s.interval_ms)
                .range(20..=60000)
                .suffix(" ms")
                .speed(5.0),
        );
        for p in INTERVAL_PRESETS {
            if ui
                .selectable_label(s.interval_ms == p, p.to_string())
                .clicked()
            {
                s.interval_ms = p;
            }
        }
    });
    let rate = tr.fmt(
        "turbo.rate",
        &[
            ("press", engine::press_ms(s.interval_ms).to_string()),
            ("rate", format!("{:.1}", 1000.0 / s.interval_ms as f64)),
        ],
    );
    ui.label(RichText::new(rate).small().weak());
    ui.horizontal(|ui| {
        ui.label(tr.t("turbo.mode"));
        ui.radio_value(&mut s.turbo_mode, TurboMode::Always, tr.t("turbo.always"));
        ui.radio_value(&mut s.turbo_mode, TurboMode::Hold, tr.t("turbo.hold"));
    });
}

/// Length and frame count of the stored macro.
fn macro_info(ui: &mut egui::Ui, sh: &Shared, tr: Tr) {
    let text = match &sh.macro_data {
        Some(m) => tr.fmt(
            "macro.info",
            &[
                ("seconds", secs(m.duration_ms)),
                ("frames", m.frames.len().to_string()),
            ],
        ),
        None => tr.t("macro.none").to_string(),
    };
    ui.label(text);
}
