//! The controller section: picker and live input view.

use super::GREEN;
use eframe::egui::{self, Color32, RichText};
use engine::Shared;
use gilrs::PowerInfo;
use i18n::Tr;
use input::pad;

/// Localized battery / cable status.
fn power_text(tr: Tr, power: &PowerInfo) -> String {
    match power {
        PowerInfo::Wired => tr.t("power.wired").to_string(),
        PowerInfo::Discharging(p) => tr.fmt("power.discharging", &[("percent", p.to_string())]),
        PowerInfo::Charging(p) => tr.fmt("power.charging", &[("percent", p.to_string())]),
        PowerInfo::Charged => tr.t("power.charged").to_string(),
        PowerInfo::Unknown => String::new(),
    }
}

/// Controller picker plus a live view of the selected controller's input.
pub(super) fn show(ui: &mut egui::Ui, sh: &mut Shared, tr: Tr) {
    let label = |i: usize| match sh.slots.iter().find(|s| s.index == i) {
        Some(s) => {
            let power = power_text(tr, &s.power);
            if power.is_empty() {
                format!("#{} {}", i + 1, s.name)
            } else {
                format!("#{} {} ({power})", i + 1, s.name)
            }
        }
        None => format!("#{} {}", i + 1, tr.t("controller.missing")),
    };
    let current = sh
        .selected.map_or_else(|| tr.t("controller.select").into(), label);
    let items: Vec<(usize, String)> = sh.slots.iter().map(|s| (s.index, label(s.index))).collect();

    let mut selected = sh.selected;
    ui.add_enabled_ui(!sh.is_running(), |ui| {
        egui::ComboBox::from_id_salt("pad")
            .selected_text(current)
            .width(ui.available_width() - 8.0)
            .show_ui(ui, |ui| {
                if items.is_empty() {
                    ui.label(tr.t("controller.none"));
                }
                for (index, text) in items {
                    ui.selectable_value(&mut selected, Some(index), text);
                }
            });
    });
    sh.selected = selected;

    let mask = sh.phys.mask();
    ui.horizontal_wrapped(|ui| {
        ui.label(tr.t("controller.input"));
        for (name, bit) in pad::BUTTONS {
            let text = RichText::new(format!(" {name} ")).monospace();
            if mask & bit != 0 {
                ui.label(text.strong().color(Color32::WHITE).background_color(GREEN));
            } else {
                ui.label(text.weak());
            }
        }
    });
    let p = &sh.phys;
    let axes = tr.fmt(
        "controller.axes",
        &[
            ("lx", format!("{:+6}", p.lx)),
            ("ly", format!("{:+6}", p.ly)),
            ("rx", format!("{:+6}", p.rx)),
            ("ry", format!("{:+6}", p.ry)),
            ("lt", format!("{:3}", p.lt)),
            ("rt", format!("{:3}", p.rt)),
        ],
    );
    ui.label(RichText::new(axes).monospace().small());
}
