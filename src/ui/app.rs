//! The window: draws the view model and hands the user's actions to the engine.

use std::time::Duration;

use eframe::egui;

use super::engine::{Command, EngineHandle};
use super::viewmodel::{Level, ViewModel};
use crate::player::PlayRequest;
use crate::state::{MAX_VOLUME, Status};

/// How long closing the window waits for the engine to stop the player.
const ENGINE_SHUTDOWN: Duration = Duration::from_secs(3);

/// The field where the user types a URI or pastes a link.
const URI_FIELD: &str = "uri-field";

/// Rows of the queue shown above the current track when the list follows it.
const ROWS_ABOVE_CURRENT: usize = 2;

pub struct GuiApp {
    view: ViewModel,
    /// `None` only while the window is being dropped.
    engine: Option<EngineHandle>,
    /// What the user is typing in the URI field.
    uri_text: String,
    /// The current row the queue list last scrolled to, to follow it when it changes.
    followed_row: Option<usize>,
    /// Frames still to draw before `--selftest` closes the window.
    selftest_frames: Option<u32>,
}

impl GuiApp {
    /// A window on `engine`, which starts by queueing `queue`.
    pub fn new(engine: EngineHandle, queue: Vec<PlayRequest>, selftest_frames: Option<u32>) -> Self {
        for request in queue {
            engine.send(Command::Add(request));
        }
        Self {
            view: ViewModel::new(),
            engine: Some(engine),
            uri_text: String::new(),
            followed_row: None,
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
        egui::Panel::top("session").show(ui, |ui| self.show_session(ui));
        egui::Panel::top("player").show(ui, |ui| {
            self.show_now_playing(ui);
            self.show_controls(ui);
            self.show_notice(ui);
        });
        egui::Panel::bottom("add").show(ui, |ui| self.show_uri_field(ui));
        egui::CentralPanel::default().show(ui, |ui| self.show_queue(ui));
    }

    /// The session, or why there is none, with what to do about it.
    fn show_session(&self, ui: &mut egui::Ui) {
        if !self.view.is_offline() {
            ui.label(self.view.connection_label());
            return;
        }
        ui.colored_label(ui.visuals().error_fg_color, self.view.connection_label());
        ui.horizontal_wrapped(|ui| {
            if ui.button("Riprova").clicked() {
                self.send(Command::Reconnect);
            }
            if self.view.needs_login() {
                ui.label("Accedi da un terminale con `spotilite login`, poi premi «Riprova».");
            }
        });
    }

    /// The latest message for the user, until it is dismissed.
    fn show_notice(&mut self, ui: &mut egui::Ui) {
        let Some(notice) = self.view.notice() else { return };
        let color = match notice.level {
            Level::Info => ui.visuals().text_color(),
            Level::Warning => ui.visuals().warn_fg_color,
            Level::Error => ui.visuals().error_fg_color,
        };
        let text = notice.text.clone();
        let mut dismissed = false;
        ui.horizontal(|ui| {
            dismissed = ui.small_button("✖").on_hover_text("Chiudi il messaggio").clicked();
            ui.add(egui::Label::new(egui::RichText::new(text).color(color)).wrap());
        });
        if dismissed {
            self.view.dismiss_notice();
        }
    }

    /// The queue, then the requests waiting for the session; the list follows the
    /// current track when it changes. Only the visible rows are worked out.
    fn show_queue(&mut self, ui: &mut egui::Ui) {
        let total = self.view.queue_len();
        if total == 0 {
            ui.weak("La coda è vuota: aggiungi un brano, un episodio o un album qui sotto.");
            return;
        }
        let row_height = ui.text_style_height(&egui::TextStyle::Body);
        let mut scroll = egui::ScrollArea::vertical().auto_shrink(false);
        let current = self.view.current_row();
        if current != self.followed_row {
            self.followed_row = current;
            if let Some(row) = current {
                let step = row_height + ui.spacing().item_spacing.y;
                scroll = scroll.vertical_scroll_offset(row.saturating_sub(ROWS_ABOVE_CURRENT) as f32 * step);
            }
        }
        scroll.show_rows(ui, row_height, total, |ui, rows| {
            for index in rows {
                let Some(row) = self.view.queue_row(index) else { continue };
                let line = format!("{}. {}", row.number, row.label);
                let text = if row.current {
                    egui::RichText::new(format!("▶ {line}"))
                        .strong()
                        .background_color(ui.visuals().selection.bg_fill)
                } else if row.pending {
                    egui::RichText::new(format!("{line} (in attesa della sessione)")).weak().italics()
                } else {
                    egui::RichText::new(line)
                };
                ui.add(egui::Label::new(text).truncate());
            }
        });
    }

    /// The field for a URI or a link, which Enter or «Aggiungi» queues.
    fn show_uri_field(&mut self, ui: &mut egui::Ui) {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let add = ui.button("Aggiungi").clicked();
            let field = egui::TextEdit::singleline(&mut self.uri_text)
                .id(egui::Id::new(URI_FIELD))
                .hint_text("spotify:track:..., spotify:album:... o link di Spotify")
                .desired_width(f32::INFINITY);
            let response = ui.add(field);
            let entered = response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            if add || entered {
                self.submit_uri();
            }
            if entered {
                // Ready for the next one.
                response.request_focus();
            }
        });
    }

    /// Queues what the user typed; when it cannot be played, the text stays for fixing
    /// and the view model shows why.
    fn submit_uri(&mut self) {
        if let Some(command) = self.view.submit_uri(&self.uri_text) {
            self.send(command);
            self.uri_text.clear();
        }
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
    const B: &str = "spotify:track:6rqhFgbbKwnb9MLmUQDhG6";

    /// A window opened with `A` to queue, as `spotilite gui <A>` does, on an engine
    /// whose session opens at once; once `A` plays.
    fn playing_window() -> (GuiApp, FakeControls) {
        let (connector, controls) = FakeConnector::connected("mario");
        let engine = EngineHandle::spawn(connector, || {}).expect("the engine must start");
        let mut app = GuiApp::new(engine, vec![PlayRequest::Item(A.to_string())], None);
        let engine = app.engine.as_ref().expect("the engine");
        pump_until(engine, &mut app.view, "A playing", |view| {
            view.now_playing().is_some_and(|now| now.title == A)
        });
        assert_eq!(controls.playback_calls(), [start(A)]);
        (app, controls)
    }

    fn start(uri: &str) -> Call {
        Call::Apply(PlaybackEffect::StartTrack(uri.to_string()))
    }

    /// Puts the keyboard focus in the URI field, as a click on it does.
    fn focus_uri_field(ctx: &egui::Context, app: &mut GuiApp) {
        frame(ctx, app, Vec::new());
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(URI_FIELD)));
        frame(ctx, app, Vec::new());
        assert!(ctx.text_edit_focused(), "the field must have the focus");
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
        assert_eq!(controls.playback_calls(), [start(A), Call::Apply(PlaybackEffect::Stop)]);
    }

    #[test]
    fn a_uri_typed_in_the_field_is_queued_with_enter() {
        let (mut app, _controls) = playing_window();
        let ctx = egui::Context::default();
        focus_uri_field(&ctx, &mut app);
        frame(&ctx, &mut app, vec![egui::Event::Text(B.to_string())]);
        assert_eq!(app.uri_text, B);

        frame(&ctx, &mut app, vec![key(egui::Key::Enter)]);
        assert_eq!(app.uri_text, "", "the field empties for the next one");
        let engine = app.engine.as_ref().expect("the engine");
        pump_until(engine, &mut app.view, "B in the queue", |view| view.queue_len() == 2);
        let rows = app.view.queue_rows();
        assert_eq!((rows[1].label.as_str(), rows[1].current), (B, false));
    }

    #[test]
    fn space_in_the_field_is_typed_not_played() {
        let (mut app, controls) = playing_window();
        let ctx = egui::Context::default();
        focus_uri_field(&ctx, &mut app);
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Space), egui::Event::Text(" ".to_string())],
        );
        assert_eq!(app.uri_text, " ");
        app.send(Command::Next);
        controls.wait_for(&Call::Apply(PlaybackEffect::Stop));
        assert_eq!(controls.playback_calls(), [start(A), Call::Apply(PlaybackEffect::Stop)], "no pause");
    }

    #[test]
    fn an_invalid_uri_stays_in_the_field_with_a_warning() {
        let (mut app, _controls) = playing_window();
        app.uri_text = "spotify:playlist:37i9dQZF1DXcBWIGoYBM5M".to_string();
        app.submit_uri();
        assert_eq!(app.uri_text, "spotify:playlist:37i9dQZF1DXcBWIGoYBM5M", "kept for fixing");
        let notice = app.view.notice().expect("a warning");
        assert_eq!(notice.level, Level::Warning);
        assert!(notice.text.contains("le playlist arriveranno"), "{}", notice.text);
        // The window draws the warning without trouble.
        frame(&egui::Context::default(), &mut app, Vec::new());
    }

    #[test]
    fn without_a_token_the_window_keeps_the_queue_and_invites_to_sign_in() {
        let (connector, _) = FakeConnector::scripted(vec![Err(crate::ui::engine::Problem::NotSignedIn)]);
        let engine = EngineHandle::spawn(connector, || {}).expect("the engine must start");
        let mut app = GuiApp::new(engine, vec![PlayRequest::Item(A.to_string())], None);
        let engine = app.engine.as_ref().expect("the engine");
        pump_until(engine, &mut app.view, "the invitation and the pending track", |view| {
            view.needs_login() && view.queue_len() == 1
        });
        assert!(app.view.queue_rows()[0].pending);
        assert!(app.view.connection_label().contains("spotilite login"));
        let ctx = egui::Context::default();
        frame(&ctx, &mut app, Vec::new());
        frame(&ctx, &mut app, vec![key(egui::Key::Space)]);
        pump_until(
            app.engine.as_ref().expect("the engine"),
            &mut app.view,
            "the refusal of space",
            |view| view.notice().is_some_and(|notice| notice.text.contains("non è collegato")),
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
