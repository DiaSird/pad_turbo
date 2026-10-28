//! Choosing where the keyboard output sends its keys: the focused window or a
//! running process (see [`output::target`]).

use super::{AMBER, GREEN};
use eframe::egui::{self, RichText};
use engine::{Shared, TargetMode};
use i18n::Tr;
use output::target::{self, ProcessInfo};

/// GUI-only state of the target process picker.
#[derive(Default)]
pub(super) struct TargetPicker {
    /// Running processes, refreshed whenever the list is opened; `None` until
    /// first listed.
    processes: Option<Vec<ProcessInfo>>,
    /// Case-insensitive text matched against executable name, title and PID.
    filter: String,
    /// Also list processes without a window (they cannot receive keys).
    show_all: bool,
    /// Whether the list was open in the previous frame.
    was_open: bool,
}

/// Keyboard destination: the focused window, or a running process picked
/// from a list, which is then driven in the background. Locked while a
/// function runs.
pub(super) fn show(ui: &mut egui::Ui, sh: &mut Shared, tr: Tr, picker: &mut TargetPicker) {
    let running = sh.is_running();
    let s = &mut sh.settings;
    ui.add_enabled_ui(!running, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(tr.t("target.label"));
            ui.radio_value(
                &mut s.target,
                TargetMode::Foreground,
                tr.t("target.foreground"),
            );
            ui.radio_value(&mut s.target, TargetMode::Process, tr.t("target.process"));
        });
        if s.target != TargetMode::Process {
            return;
        }
        ui.horizontal(|ui| {
            ui.label(tr.t("target.filter"));
            ui.add(egui::TextEdit::singleline(&mut picker.filter).desired_width(140.0));
            ui.checkbox(&mut picker.show_all, tr.t("target.show_all"));
        });
        ui.horizontal(|ui| {
            let current = match (s.target_exe.as_str(), s.target_pid) {
                ("", _) => tr.t("target.select").to_string(),
                (exe, Some(pid)) => format!("{exe}  (PID {pid})"),
                (exe, None) => tr.fmt("target.all_same", &[("exe", exe.to_string())]),
            };
            let filter = picker.filter.to_lowercase();
            let list = picker.processes.get_or_insert_with(target::list_processes);
            let open = egui::ComboBox::from_id_salt("target")
                .selected_text(current)
                .width(ui.available_width() - 80.0)
                .height(320.0)
                .show_ui(ui, |ui| {
                    let mut shown = 0;
                    for p in list.iter().filter(|p| picker.show_all || p.title.is_some()) {
                        let text = match &p.title {
                            Some(title) => format!("{}  PID {} — {title}", p.exe, p.pid),
                            None => {
                                format!("{}  PID {}  {}", p.exe, p.pid, tr.t("target.no_window"))
                            }
                        };
                        if !filter.is_empty() && !text.to_lowercase().contains(&filter) {
                            continue;
                        }
                        shown += 1;
                        let selected = s.target_pid == Some(p.pid);
                        let label = if p.title.is_some() {
                            RichText::new(text)
                        } else {
                            RichText::new(text).weak()
                        };
                        if ui.selectable_label(selected, label).clicked() {
                            s.target_exe.clone_from(&p.exe);
                            s.target_pid = Some(p.pid);
                        }
                    }
                    if shown == 0 {
                        ui.label(tr.t("target.none"));
                    }
                })
                .inner
                .is_some();
            let refresh = ui.button(tr.t("target.refresh")).clicked();
            // Opening the list re-reads the running processes.
            if refresh || (open && !picker.was_open) {
                *list = target::list_processes();
            }
            picker.was_open = open;
        });
        if let Some(pid) = s.target_pid {
            let text = tr.fmt("target.any_pid", &[("exe", s.target_exe.clone())]);
            if ui
                .small_button(text)
                .on_hover_text(format!("PID {pid}"))
                .clicked()
            {
                s.target_pid = None;
            }
        }
    });

    let s = &sh.settings;
    if s.target != TargetMode::Process || s.target_exe.is_empty() {
        return;
    }
    let exe = s.target_exe.clone();
    if let (Some(pid), Some((pids, _))) = (s.target_pid, &sh.target_status) {
        if !pids.is_empty() && !pids.contains(&pid) {
            ui.colored_label(
                AMBER,
                tr.fmt(
                    "target.pid_gone",
                    &[("pid", pid.to_string()), ("exe", exe.clone())],
                ),
            );
        }
    }
    match &sh.target_status {
        Some((pids, n)) if *n > 0 => {
            let pids = pids
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            ui.colored_label(
                GREEN,
                tr.fmt(
                    "target.windows",
                    &[("pids", pids), ("count", n.to_string())],
                ),
            );
        }
        Some((pids, _)) if !pids.is_empty() => {
            let pids = pids
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            ui.colored_label(
                AMBER,
                tr.fmt("target.windowless", &[("exe", exe), ("pids", pids)]),
            );
        }
        _ => {
            ui.colored_label(AMBER, tr.fmt("target.missing", &[("exe", exe)]));
        }
    }
    ui.label(RichText::new(tr.t("target.hint")).small().weak());
}
