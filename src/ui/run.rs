//! The bottom run bar (Start/Stop, status, countdown) and the log.

use super::{AMBER, GREEN, RED, secs};
use eframe::egui::{self, Color32, RichText};
use engine::{Feature, Shared};
use i18n::Tr;
use input::pad;
use std::time::Instant;

/// Bottom bar: Start/Stop button, run status, countdown option and the toggle
/// hint.
pub(super) fn bar(ui: &mut egui::Ui, sh: &mut Shared, tr: Tr) {
    let f = sh.settings.feature;
    let running = sh.is_running();

    ui.horizontal(|ui| {
        let (label, color) = if running {
            (tr.t("run.stop"), RED)
        } else {
            (tr.t("run.start"), GREEN)
        };
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
                &[
                    ("feature", tr.t(pf.key()).into()),
                    ("seconds", format!("{left:.1}")),
                ],
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
                Feature::Record => tr.fmt(
                    "run.detail.frames",
                    &[("frames", sh.rec_frames.to_string())],
                ),
                Feature::Play => match &sh.macro_data {
                    Some(m) => tr.fmt(
                        "run.detail.play",
                        &[
                            ("position", secs(sh.play_pos_ms)),
                            ("total", secs(m.duration_ms)),
                        ],
                    ),
                    None => String::new(),
                },
            };
            let text = tr.fmt(
                "run.active",
                &[
                    ("feature", tr.t(af.key()).into()),
                    ("seconds", secs(sh.run_ms)),
                    ("detail", detail),
                ],
            );
            ui.colored_label(GREEN, text);
        } else if sh.check(f).is_ok() {
            ui.label(
                RichText::new(tr.fmt("run.idle", &[("feature", tr.t(f.key()).into())])).weak(),
            );
        }
    });

    ui.horizontal(|ui| {
        ui.checkbox(&mut sh.settings.countdown, tr.t("run.countdown"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let hint = tr.fmt("run.hint", &[("key", sh.binding.label().to_string())]);
            ui.label(RichText::new(hint).small().weak())
                .on_hover_text(tr.t("run.config_tip"));
        });
    });
}

/// Recent log lines, translated on every frame.
pub(super) fn log(ui: &mut egui::Ui, sh: &Shared, tr: Tr) {
    egui::CollapsingHeader::new(tr.t("section.log"))
        .default_open(true)
        .show(ui, |ui| {
            for line in &sh.log {
                ui.label(RichText::new(tr.msg(line)).small());
            }
        });
}
