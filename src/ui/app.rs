//! The window: draws the view model and hands the user's actions to the engine.

use std::time::Duration;

use eframe::egui;

use super::engine::{Command, EngineHandle};
use super::viewmodel::ViewModel;
use crate::state::{MAX_VOLUME, Status};

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

    fn send(&self, command: Command) {
        if let Some(engine) = &self.engine {
            engine.send(command);
        }
    }

    /// Draws the whole window.
    fn show(&mut self, ui: &mut egui::Ui) {
        self.handle_keys(ui.ctx());
        egui::Panel::top("session").show(ui, |ui| {
            ui.label(self.view.connection_label());
        });
        egui::Panel::top("player").show(ui, |ui| {
            self.show_now_playing(ui);
            self.show_controls(ui);
        });
        egui::CentralPanel::default().show(ui, |_ui| {});
    }

    /// Space plays and pauses, unless the user is typing. The key is taken before the
    /// widgets see it, so a focused button does not react to it as well.
    fn handle_keys(&self, ctx: &egui::Context) {
        if ctx.text_edit_focused() {
            return;
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Space)) {
            self.send(Command::TogglePlay);
        }
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

    fn show_controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("⏮").on_hover_text("Brano precedente").clicked() {
                self.send(Command::Prev);
            }
            let symbol = if self.view.status() == Status::Playing { "⏸" } else { "▶" };
            let play = ui.button(format!("{symbol} {}", self.view.play_label()));
            if play.on_hover_text("Barra spaziatrice").clicked() {
                self.send(Command::TogglePlay);
            }
            if ui.button("⏭").on_hover_text("Brano successivo").clicked() {
                self.send(Command::Next);
            }
            ui.separator();
            let mut volume = self.view.volume();
            let slider = egui::Slider::new(&mut volume, 0..=MAX_VOLUME).text("Volume");
            if ui.add(slider).changed() {
                let command = self.view.set_volume(volume);
                self.send(command);
            }
        });
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::PlaybackEvent;
    use crate::state::PlaybackEffect;
    use crate::ui::fake::{Call, FakeConnector, FakeControls, pump_until};

    const A: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQC";

    /// A window on an engine with an open session, playing `A`.
    fn playing_window() -> (GuiApp, FakeControls) {
        let (connector, controls) = FakeConnector::connected("mario");
        let engine = EngineHandle::spawn(connector, || {}).expect("the engine must start");
        let mut app = GuiApp::new(engine, None);
        let engine = app.engine.as_ref().expect("the engine");
        pump_until(engine, &mut app.view, "the session", |view| {
            view.connection_label().contains("mario")
        });
        let add = app.view.submit_uri(A).expect("a valid URI");
        app.send(add);
        controls.wait_for(&Call::Apply(PlaybackEffect::StartTrack(A.to_string())));
        (app, controls)
    }

    fn key(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    /// Draws one frame of `app` without a window, with `events` as the user's input.
    fn frame(ctx: &egui::Context, app: &mut GuiApp, events: Vec<egui::Event>) {
        let input = egui::RawInput {
            events,
            ..egui::RawInput::default()
        };
        ctx.run_ui(input, |ui| {
            app.receive_events();
            app.show(ui);
        })
        .drop_without_applying_deltas();
    }

    #[test]
    fn space_pauses_and_plays() {
        let (mut app, controls) = playing_window();
        let ctx = egui::Context::default();
        frame(&ctx, &mut app, Vec::new());

        frame(&ctx, &mut app, vec![key(egui::Key::Space)]);
        controls.wait_for(&Call::Apply(PlaybackEffect::Pause));

        frame(&ctx, &mut app, vec![key(egui::Key::Space)]);
        controls.wait_for(&Call::Apply(PlaybackEffect::Resume));
    }

    #[test]
    fn other_keys_leave_playback_alone() {
        let (mut app, controls) = playing_window();
        let ctx = egui::Context::default();
        frame(&ctx, &mut app, vec![key(egui::Key::Enter), key(egui::Key::A)]);
        // A later command shows that the engine has gone past the keys.
        app.send(Command::Next);
        controls.wait_for(&Call::Apply(PlaybackEffect::Stop));
        assert_eq!(
            controls.playback_calls(),
            [
                Call::Apply(PlaybackEffect::StartTrack(A.to_string())),
                Call::Apply(PlaybackEffect::Stop)
            ]
        );
    }

    #[test]
    fn the_window_draws_what_the_engine_reports() {
        let (mut app, controls) = playing_window();
        controls.emit(PlaybackEvent::TrackInfo {
            uri: A.to_string(),
            title: "Never Gonna Give You Up".to_string(),
            artists: vec!["Rick Astley".to_string()],
            duration: Duration::from_secs(213),
        });
        let ctx = egui::Context::default();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while app.view.now_playing().is_none_or(|now| now.title != "Never Gonna Give You Up") {
            assert!(std::time::Instant::now() < deadline, "the track info never arrived");
            frame(&ctx, &mut app, Vec::new());
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(app.view.status(), Status::Playing);
    }
}
