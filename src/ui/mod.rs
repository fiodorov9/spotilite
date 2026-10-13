//! Graphical interface: the window, its view model and the engine thread that plays.
//!
//! The window never touches the network or the audio device. It sends
//! [`engine::Command`]s to the engine thread and draws what the
//! [`viewmodel::ViewModel`] makes of the [`engine::EngineEvent`]s that come back.

mod app;
pub mod engine;
pub mod viewmodel;

use std::io::{self, Write};
use std::time::Duration;
use std::{process, thread};

use eframe::egui;
use thiserror::Error;

use self::app::GuiApp;
use self::engine::{EngineHandle, NoConnection, SpotifyConnector};

/// Title of the window.
pub const TITLE: &str = "spotilite";

/// How long `--selftest` may take on top of [`SELFTEST_FRAME_ALLOWANCE`] per frame: the
/// window may never show up, and the check must end anyway.
const SELFTEST_GRACE: Duration = Duration::from_secs(30);
const SELFTEST_FRAME_ALLOWANCE: Duration = Duration::from_millis(100);

#[derive(Debug, Error)]
pub enum GuiError {
    #[error("impossibile aprire la finestra: {0}")]
    Window(String),
}

/// How `spotilite gui` opens the window.
pub struct GuiOptions {
    /// How the engine opens the Spotify session.
    pub connector: SpotifyConnector,
    /// Draw this many frames without connecting to Spotify, then close: the automatic
    /// check of `spotilite gui --selftest`.
    pub selftest: Option<u32>,
}

/// Opens the window and returns once it has been closed.
///
/// OpenGL is the lighter way to draw it, but some displays only offer OpenGL 1.1 (virtual
/// displays, some remote sessions): there the window falls back to Direct3D or Vulkan,
/// through wgpu.
pub fn run(options: GuiOptions) -> Result<(), GuiError> {
    let GuiOptions { connector, selftest } = options;
    if let Some(frames) = selftest {
        start_selftest_watchdog(frames);
    }
    let opengl_error = match open_window(eframe::Renderer::Glow, &connector, selftest) {
        Err(err @ (eframe::Error::Glutin(_) | eframe::Error::NoGlutinConfigs(..) | eframe::Error::OpenGL(_))) => err,
        result => return result.map_err(|err| GuiError::Window(err.to_string())),
    };
    open_window(eframe::Renderer::Wgpu, &connector, selftest)
        .map_err(|err| GuiError::Window(format!("{err} (OpenGL: {opengl_error})")))
}

/// Opens the window with `renderer` and starts the engine once the window exists.
fn open_window(
    renderer: eframe::Renderer,
    connector: &SpotifyConnector,
    selftest: Option<u32>,
) -> Result<(), eframe::Error> {
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(TITLE)
            .with_app_id(TITLE)
            // Small enough for a 640x480 screen with its taskbar.
            .with_inner_size([420.0, 400.0])
            .with_min_inner_size([320.0, 280.0])
            // The system's default icon rather than egui's.
            .with_icon(egui::IconData::default()),
        renderer,
        wgpu_options: eframe::WgpuConfiguration {
            wgpu_setup: eframe::egui_wgpu::WgpuSetup::CreateNew(wgpu_setup()),
            ..eframe::WgpuConfiguration::default()
        },
        ..eframe::NativeOptions::default()
    };
    let connector = connector.clone();
    eframe::run_native(
        TITLE,
        native,
        Box::new(move |creation| {
            // The engine wakes the window up at every event.
            let ctx = creation.egui_ctx.clone();
            let wake = move || ctx.request_repaint();
            let engine = match selftest {
                Some(_) => EngineHandle::spawn(NoConnection, wake)?,
                None => EngineHandle::spawn(connector, wake)?,
            };
            Ok(Box::new(GuiApp::new(engine, selftest)))
        }),
    )
}

/// How wgpu picks the graphics card and the API, when OpenGL cannot be used.
fn wgpu_setup() -> eframe::egui_wgpu::WgpuSetupCreateNew {
    use eframe::wgpu::{Backends, PowerPreference};

    let mut setup = eframe::egui_wgpu::WgpuSetupCreateNew::without_display_handle();
    // A music player has no use for the most powerful GPU of a laptop.
    setup.power_preference = PowerPreference::LowPower;
    // Direct3D 12 is the native API on Windows. Vulkan, which wgpu would pick first, can
    // leave the window blank on a virtual display, without any error. `WGPU_BACKEND`
    // still has the last word.
    if cfg!(windows) && Backends::from_env().is_none() {
        setup.instance_descriptor.backends = Backends::DX12;
    }
    setup
}

/// Ends the process with a failure if `--selftest` has not closed the window in time,
/// for instance because there is no graphical session to show it in.
fn start_selftest_watchdog(frames: u32) {
    let limit = SELFTEST_GRACE + SELFTEST_FRAME_ALLOWANCE * frames;
    let watchdog = move || {
        thread::sleep(limit);
        let _ = writeln!(
            io::stderr(),
            "errore: prova automatica: la finestra non ha disegnato {frames} frame entro {} s",
            limit.as_secs()
        );
        process::exit(1);
    };
    // Without the watchdog the check still runs; it just cannot be cut short.
    let _ = thread::Builder::new().name("spotilite-selftest".to_string()).spawn(watchdog);
}
