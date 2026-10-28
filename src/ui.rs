//! The egui front end.
//!
//! The GUI only edits [`Settings`] and reads status from [`Shared`]; all timing
//! work happens on the worker thread (see [`crate::engine`]). The selected
//! function is started and stopped either with the Start/Stop button at the
//! bottom of the window or with the toggle binding from `config.json`.
//!
//! Every visible string goes through [`Tr`], which renders it in the language
//! chosen in the header combo box (English by default).

use crate::binding::{Binding, Config, DEFAULT_TOGGLE};
use crate::engine::{self, Feature, OutputPref, Settings, Shared, TurboMode};
use crate::i18n::{Language, Message, Tr, Translations};
use crate::{output, pad};
use eframe::egui::{self, Color32, RichText};
use gilrs::PowerInfo;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Interval presets offered next to the turbo interval field, in milliseconds.
const INTERVAL_PRESETS: [u32; 6] = [50, 100, 250, 500, 1000, 2000];
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
            let sh = shared.lock().unwrap();
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
        }
    }
}

impl Drop for App {
    /// Asks the worker to quit and waits for it, so that outputs are released
    /// (no stuck keys, virtual controller unplugged) before the process exits.
    fn drop(&mut self) {
        self.shared.lock().unwrap().quit = true;
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.ctx().request_repaint_after(REPAINT_INTERVAL);
        let mut guard = self.shared.lock().unwrap();
        let sh = &mut *guard;
        let tr = self.translations.tr(sh.settings.language);

        // Start/Stop stays visible at the bottom regardless of scrolling.
        egui::Panel::bottom("run").resizable(false).show(ui, |ui| {
            ui.add_space(6.0);
            run_section(ui, sh, tr);
            ui.add_space(4.0);
        });
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                header(ui, sh, &self.translations, tr);
                section(ui, tr.t("section.controller"), |ui| controller_section(ui, sh, tr));
                section(ui, tr.t("section.output"), |ui| output_section(ui, sh, tr));
                section(ui, tr.t("section.toggle"), |ui| {
                    toggle_section(ui, sh, tr, &mut self.binding_text, &mut self.binding_error)
                });
                section(ui, tr.t("section.feature"), |ui| feature_section(ui, sh, tr));
                log_section(ui, sh, tr);
            });
        });

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
            let config = Config { toggle: label.clone() };
            let msg = match config.save(&self.config_path) {
                Ok(()) => Message::new("log.toggle_changed").text("key", &label),
                Err(e) => Message::new("log.toggle_save_failed").text("key", &label).text("error", e),
            };
            sh.log(msg);
            self.binding_text = label.clone();
            self.saved_binding = label;
        }
    }
}

/// Adds the first available CJK system font as a fallback for all families.
fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    if let Some(bytes) = CJK_FONTS.iter().find_map(|p| std::fs::read(p).ok()) {
        fonts.font_data.insert("cjk".into(), Arc::new(egui::FontData::from_owned(bytes)));
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
            egui::ComboBox::from_id_salt("language").selected_text(name(*lang)).show_ui(ui, |ui| {
                for l in Language::ALL {
                    ui.selectable_value(lang, l, name(l));
                }
            });
            ui.label(tr.t("language.label"));
        });
    });
    ui.add_space(4.0);
}

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
fn controller_section(ui: &mut egui::Ui, sh: &mut Shared, tr: Tr) {
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
    let current = sh.selected.map(label).unwrap_or_else(|| tr.t("controller.select").into());
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

/// Output device choice, its status, and the keyboard mapping table.
fn output_section(ui: &mut egui::Ui, sh: &mut Shared, tr: Tr) {
    ui.add_enabled_ui(!sh.is_running(), |ui| {
        ui.horizontal(|ui| {
            ui.radio_value(&mut sh.settings.output, OutputPref::ViGEm, tr.t("output.vigem"));
            ui.radio_value(&mut sh.settings.output, OutputPref::Keyboard, tr.t("output.keyboard"));
        });
    });
    let ok = sh.output_is_vigem || sh.settings.output == OutputPref::Keyboard;
    ui.colored_label(if ok { GREEN } else { RED }, tr.msg(&sh.output_status));
    if sh.settings.output == OutputPref::ViGEm && !sh.output_is_vigem {
        ui.label(RichText::new(tr.t("output.driver_hint")).small());
    }
    if sh.output_is_vigem {
        ui.label(RichText::new(tr.t("output.passthrough_hint")).small().weak());
    } else {
        egui::CollapsingHeader::new(tr.t("output.keymap")).show(ui, |ui| {
            egui::Grid::new("keymap").num_columns(4).striped(true).show(ui, |ui| {
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

/// Shows and edits the start/stop toggle binding.
///
/// Three ways to change it: record it by pressing the inputs, type it into
/// the text field (syntax in [`crate::binding`]), or reset it to
/// [`DEFAULT_TOGGLE`]. Editing is disabled while a function runs so that the
/// binding cannot change under the user's fingers.
fn toggle_section(ui: &mut egui::Ui, sh: &mut Shared, tr: Tr, text: &mut String, error: &mut Option<String>) {
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
            let field = ui.add(egui::TextEdit::singleline(text).desired_width(200.0).hint_text("Ctrl+F5"));
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

/// Feature tabs and the settings of the selected feature.
fn feature_section(ui: &mut egui::Ui, sh: &mut Shared, tr: Tr) {
    ui.add_enabled_ui(!sh.is_running(), |ui| {
        ui.horizontal(|ui| {
            for f in Feature::ALL {
                ui.selectable_value(&mut sh.settings.feature, f, RichText::new(tr.t(f.key())).size(14.0));
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
    egui::Grid::new("turbo_buttons").num_columns(4).show(ui, |ui| {
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
    ui.label(tr.fmt("turbo.selected", &[("buttons", pad::format_mask(s.turbo_mask))]));

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(tr.t("turbo.interval"));
        ui.add(egui::DragValue::new(&mut s.interval_ms).range(20..=60000).suffix(" ms").speed(5.0));
        for p in INTERVAL_PRESETS {
            if ui.selectable_label(s.interval_ms == p, p.to_string()).clicked() {
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
            &[("seconds", secs(m.duration_ms)), ("frames", m.frames.len().to_string())],
        ),
        None => tr.t("macro.none").to_string(),
    };
    ui.label(text);
}

/// Bottom bar: Start/Stop button, run status, countdown option and the toggle
/// hint.
fn run_section(ui: &mut egui::Ui, sh: &mut Shared, tr: Tr) {
    let f = sh.settings.feature;
    let running = sh.is_running();

    ui.horizontal(|ui| {
        let (label, color) = if running { (tr.t("run.stop"), RED) } else { (tr.t("run.start"), GREEN) };
        let button = egui::Button::new(RichText::new(label).size(14.0).color(Color32::WHITE))
            .fill(color)
            .min_size(egui::vec2(88.0, 28.0));
        if running {
            if ui.add(button).clicked() {
                sh.stop();
            }
        } else {
            let check = sh.check(f);
            if ui.add_enabled(check.is_ok(), button).clicked() {
                sh.request_start(f);
            }
            if let Err(reason) = check {
                ui.colored_label(AMBER, tr.t(reason));
            }
        }

        if let Some((pf, at)) = sh.pending {
            let left = at.saturating_duration_since(Instant::now()).as_secs_f32();
            ui.label(tr.fmt(
                "run.pending",
                &[("feature", tr.t(pf.key()).into()), ("seconds", format!("{left:.1}"))],
            ));
        } else if let Some(af) = sh.active {
            let detail = match af {
                Feature::Turbo => tr.fmt(
                    "run.detail.turbo",
                    &[
                        ("buttons", pad::format_mask(sh.settings.turbo_mask)),
                        ("interval", sh.settings.interval_ms.to_string()),
                    ],
                ),
                Feature::Record => tr.fmt("run.detail.frames", &[("frames", sh.rec_frames.to_string())]),
                Feature::Play => match &sh.macro_data {
                    Some(m) => tr.fmt(
                        "run.detail.play",
                        &[("position", secs(sh.play_pos_ms)), ("total", secs(m.duration_ms))],
                    ),
                    None => String::new(),
                },
            };
            let text = tr.fmt(
                "run.active",
                &[("feature", tr.t(af.key()).into()), ("seconds", secs(sh.run_ms)), ("detail", detail)],
            );
            ui.colored_label(GREEN, text);
        } else if sh.check(f).is_ok() {
            ui.label(RichText::new(tr.fmt("run.idle", &[("feature", tr.t(f.key()).into())])).weak());
        }
    });

    ui.horizontal(|ui| {
        ui.checkbox(&mut sh.settings.countdown, tr.t("run.countdown"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let hint = tr.fmt("run.hint", &[("key", sh.binding.label().to_string())]);
            ui.label(RichText::new(hint).small().weak()).on_hover_text(tr.t("run.config_tip"));
        });
    });
}

/// Recent log lines, translated on every frame.
fn log_section(ui: &mut egui::Ui, sh: &Shared, tr: Tr) {
    egui::CollapsingHeader::new(tr.t("section.log")).default_open(true).show(ui, |ui| {
        for line in &sh.log {
            ui.label(RichText::new(tr.msg(line)).small());
        }
    });
}

/// Milliseconds as seconds with one decimal.
fn secs(ms: u64) -> String {
    format!("{:.1}", ms as f64 / 1000.0)
}
