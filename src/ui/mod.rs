//! The egui front end.
//!
//! The GUI only edits [`Settings`] and reads status from [`Shared`]; all timing
//! work happens on the worker thread (see [`engine`]). The selected
//! function is started and stopped either with the Start/Stop button at the
//! bottom of the window or with the toggle binding from `config.json`.
//!
//! Every visible string goes through [`Tr`], which renders it in the language
//! chosen in the header combo box (English by default).
//!
//! This module holds the [`App`] itself and the helpers shared by the
//! sections; each section of the window lives in its own submodule.

mod controller;
mod feature;
mod keymap_editor;
mod output_device;
mod run;
mod target_picker;
mod toggle;

use eframe::egui::{self, Color32, RichText};
use engine::{Settings, Shared};
use i18n::{Language, Message, Tr, Translations};
use input::binding::Config;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use target_picker::TargetPicker;

/// Accent for "running" / "pressed" / "OK".
const GREEN: Color32 = Color32::from_rgb(46, 140, 80);
/// Accent for errors.
const RED: Color32 = Color32::from_rgb(190, 60, 60);
/// Accent for warnings that block starting.
const AMBER: Color32 = Color32::from_rgb(220, 160, 40);
/// How often the window refreshes to show live controller input.
const REPAINT_INTERVAL: Duration = Duration::from_millis(50);

/// Windows fonts tried, in order, to supply Japanese glyphs.
///
/// egui's built-in fonts cover Latin only. The first font that exists is
/// appended as a fallback, so Latin text keeps egui's look.
const CJK_FONTS: [&str; 3] = [
    "C:/Windows/Fonts/YuGothM.ttc",
    "C:/Windows/Fonts/meiryo.ttc",
    "C:/Windows/Fonts/msgothic.ttc",
];

/// The eframe application.
pub struct App {
    /// State shared with the worker thread.
    shared: Arc<Mutex<Shared>>,
    /// Worker thread handle, joined on drop.
    worker: Option<JoinHandle<()>>,
    /// Where [`Settings`] are persisted.
    settings_path: PathBuf,
    /// Last settings written to disk, to save only on change.
    saved: Settings,
    /// Where the toggle binding is persisted.
    config_path: PathBuf,
    /// Label of the binding last written to `config.json`.
    saved_binding: String,
    /// Contents of the binding text field.
    binding_text: String,
    /// Offending token of the last rejected text, shown under the field.
    binding_error: Option<String>,
    /// UI string tables.
    translations: Translations,
    /// State of the target process picker.
    picker: TargetPicker,
}

impl App {
    /// Creates the application and installs fonts on the egui context.
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        shared: Arc<Mutex<Shared>>,
        worker: JoinHandle<()>,
        settings_path: PathBuf,
        config_path: PathBuf,
        translations: Translations,
    ) -> Self {
        setup_fonts(&cc.egui_ctx);
        let (saved, label) = {
            let sh = engine::lock(&shared);
            (sh.settings.clone(), sh.binding.label().to_string())
        };
        Self {
            shared,
            worker: Some(worker),
            settings_path,
            saved,
            config_path,
            saved_binding: label.clone(),
            binding_text: label,
            binding_error: None,
            translations,
            picker: TargetPicker::default(),
        }
    }
}

impl Drop for App {
    /// Asks the worker to quit and waits for it, so that outputs are released
    /// (no stuck keys, virtual controller unplugged) before the process exits.
    fn drop(&mut self) {
        engine::lock(&self.shared).quit = true;
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.ctx().request_repaint_after(REPAINT_INTERVAL);
        // Lock through a local handle so that `self` stays free; the lock is
        // held for this one frame.
        let shared = Arc::clone(&self.shared);
        self.frame(ui, &mut engine::lock(&shared));
    }
}

impl App {
    /// Draws one frame from `sh` and saves what the user changed.
    fn frame(&mut self, ui: &mut egui::Ui, sh: &mut Shared) {
        let tr = self.translations.tr(sh.settings.language);

        // Start/Stop stays visible at the bottom regardless of scrolling.
        egui::Panel::bottom("run").resizable(false).show(ui, |ui| {
            ui.add_space(6.0);
            run::bar(ui, sh, tr);
            ui.add_space(4.0);
        });
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    header(ui, sh, &self.translations, tr);
                    section(ui, tr.t("section.controller"), |ui| {
                        controller::show(ui, sh, tr);
                    });
                    section(ui, tr.t("section.output"), |ui| {
                        output_device::show(ui, sh, tr, &mut self.picker);
                    });
                    section(ui, tr.t("section.toggle"), |ui| {
                        toggle::show(ui, sh, tr, &mut self.binding_text, &mut self.binding_error);
                    });
                    section(ui, tr.t("section.feature"), |ui| feature::show(ui, sh, tr));
                    run::log(ui, sh, tr);
                });
        });
        self.persist(sh);
    }

    /// Writes settings and the toggle binding to disk when they changed.
    fn persist(&mut self, sh: &mut Shared) {
        if sh.settings != self.saved {
            if let Ok(json) = serde_json::to_string_pretty(&sh.settings) {
                let _ = std::fs::write(&self.settings_path, json);
            }
            self.saved = sh.settings.clone();
        }

        // The binding may have been changed by the text field, the reset
        // button or a capture finished by the worker.
        let label = sh.binding.label().to_string();
        if label != self.saved_binding {
            let config = Config {
                toggle: label.clone(),
            };
            let msg = match config.save(&self.config_path) {
                Ok(()) => Message::new("log.toggle_changed").text("key", &label),
                Err(e) => Message::new("log.toggle_save_failed")
                    .text("key", &label)
                    .text("error", e),
            };
            sh.log(msg);
            self.binding_text.clone_from(&label);
            self.saved_binding = label;
        }
    }
}

/// Adds the first available CJK system font as a fallback for all families.
fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    if let Some(bytes) = CJK_FONTS.iter().find_map(|p| std::fs::read(p).ok()) {
        fonts
            .font_data
            .insert("cjk".into(), Arc::new(egui::FontData::from_owned(bytes)));
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts.families.entry(family).or_default().push("cjk".into());
        }
    }
    ctx.set_fonts(fonts);
}

/// Draws a titled, rounded frame spanning the full width.
fn section(ui: &mut egui::Ui, title: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::group(ui.style())
        .inner_margin(egui::Margin::same(10))
        .corner_radius(6)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(title).strong().size(15.0));
            ui.add_space(4.0);
            add_contents(ui);
        });
    ui.add_space(8.0);
}

/// Top row: language selector, right-aligned.
///
/// Each language is listed under its own name (`language.name` from its own
/// table), so users can find their language regardless of the current one.
///
/// The right-to-left layout is nested in a horizontal row: on its own it would
/// claim all remaining height of the scroll area and push the sections below
/// out of view.
fn header(ui: &mut egui::Ui, sh: &mut Shared, translations: &Translations, tr: Tr) {
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let lang = &mut sh.settings.language;
            let name = |l: Language| translations.tr(l).t("language.name");
            egui::ComboBox::from_id_salt("language")
                .selected_text(name(*lang))
                .show_ui(ui, |ui| {
                    for l in Language::ALL {
                        ui.selectable_value(lang, l, name(l));
                    }
                });
            ui.label(tr.t("language.label"));
        });
    });
    ui.add_space(4.0);
}

/// Milliseconds as seconds with one decimal.
fn secs(ms: u64) -> String {
    format!("{:.1}", ms as f64 / 1000.0)
}
