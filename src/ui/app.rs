//! The window: draws the view model and hands the user's actions to the engine.

use std::time::Duration;

use eframe::egui;

use super::engine::EngineHandle;
use super::viewmodel::ViewModel;

/// How long closing the window waits for the engine to stop the player.
const ENGINE_SHUTDOWN: Duration = Duration::from_secs(3);

pub struct GuiApp {
    view: ViewModel,
    /// `None` only while the window is being dropped.
    engine: Option<EngineHandle>,
    /// Frames still to draw before `--selftest` closes the window.
    selftest_frames: Option<u32>,
}

impl GuiApp {
    pub fn new(engine: EngineHandle, selftest_frames: Option<u32>) -> Self {
        Self {
            view: ViewModel::new(),
            engine: Some(engine),
            selftest_frames,
        }
    }

    /// Takes in the events the engine has sent since the last frame.
    fn receive_events(&mut self) {
        let Some(engine) = &self.engine else { return };
        while let Some(event) = engine.try_event() {
            self.view.apply(event);
        }
    }

    /// Draws the whole window.
    fn show(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("session").show(ui, |ui| {
            ui.label(self.view.connection_label());
        });
        egui::CentralPanel::default().show(ui, |ui| self.show_now_playing(ui));
    }

    fn show_now_playing(&self, ui: &mut egui::Ui) {
        match self.view.now_playing() {
            Some(now) => {
                ui.heading(now.title);
                if !now.artists.is_empty() {
                    ui.label(now.artists);
                }
            }
            None => {
                ui.heading("Nessun brano in ascolto");
            }
        }
        ui.label(self.view.status_label());
        ui.add(egui::ProgressBar::new(self.view.progress()).text(self.view.position_label()));
    }

    /// Counts a frame of `--selftest`, and closes the window after the last one.
    fn count_selftest_frame(&mut self, ctx: &egui::Context) {
        let Some(left) = &mut self.selftest_frames else { return };
        *left = left.saturating_sub(1);
        if *left == 0 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            ctx.request_repaint();
        }
    }
}

impl eframe::App for GuiApp {
    fn logic(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.receive_events();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
        self.count_selftest_frame(ui.ctx());
    }
}

impl Drop for GuiApp {
    fn drop(&mut self) {
        if let Some(engine) = self.engine.take() {
            engine.shutdown(ENGINE_SHUTDOWN);
        }
    }
}
