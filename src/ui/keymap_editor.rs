//! Editing the keyboard output's key table: presets plus a key per input.

use eframe::egui::{self, RichText};
use engine::Shared;
use i18n::Tr;
use input::binding::{key_name, keys};
use output::{CONTROLS, KeyMap, KeyPreset};

/// Height of the key lists, so that the ~80 keys scroll instead of running
/// off the window.
const LIST_HEIGHT: f32 = 260.0;

/// The key table: preset buttons and one key list per controller input.
/// Locked while a function runs, like the other output settings.
pub(super) fn show(ui: &mut egui::Ui, sh: &mut Shared, tr: Tr) {
    let running = sh.is_running();
    let keymap = &mut sh.settings.keymap;
    ui.add_enabled_ui(!running, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(tr.t("keymap.preset"));
            for p in KeyPreset::ALL {
                if ui.selectable_label(keymap.is_preset(p), tr.t(p.key())).clicked() {
                    *keymap = KeyMap::preset(p);
                }
            }
        });
        egui::Grid::new("keymap").num_columns(4).striped(true).show(ui, |ui| {
            for (i, (_, control)) in CONTROLS.iter().enumerate() {
                ui.label(tr.get(&format!("control.{control}")).unwrap_or(control));
                key_list(ui, keymap, i, tr);
                if i % 2 == 1 {
                    ui.end_row();
                }
            }
        });
    });
    ui.label(RichText::new(tr.t("keymap.hint")).small().weak());
}

/// The key list of entry `i` of [`CONTROLS`].
fn key_list(ui: &mut egui::Ui, keymap: &mut KeyMap, i: usize, tr: Tr) {
    let none = tr.t("keymap.none");
    let current = keymap.key(i);
    let text = current.and_then(key_name).map_or_else(|| RichText::new(none).weak(), |n| RichText::new(n).strong());
    egui::ComboBox::from_id_salt(("keymap", i))
        .selected_text(text)
        .width(96.0)
        .height(LIST_HEIGHT)
        .show_ui(ui, |ui| {
            if ui.selectable_label(current.is_none(), none).clicked() {
                keymap.set(i, None);
            }
            for vk in keys() {
                let Some(name) = key_name(vk) else { continue };
                if ui.selectable_label(current == Some(vk), name).clicked() {
                    keymap.set(i, Some(vk));
                }
            }
        });
}
