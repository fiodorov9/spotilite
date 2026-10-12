//! Audio output: librespot's default backend, opened without panicking.
//!
//! librespot's rodio backend unwraps the device errors, so a computer without an
//! output device would crash the player thread. The device is checked here first,
//! and the backend runs under `catch_unwind` as a second line of defence: either
//! way the caller gets an [`AudioError`] with a readable message.

use std::any::Any;
use std::cell::Cell;
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Once, mpsc};

use librespot::playback::audio_backend::{self, Sink, SinkBuilder, SinkError, SinkResult};
use librespot::playback::config::AudioFormat;
use librespot::playback::convert::Converter;
use librespot::playback::decoder::AudioPacket;
use rodio::cpal::traits::{DeviceTrait, HostTrait};
use thiserror::Error;

/// Name of librespot's rodio backend, the default one.
const RODIO_BACKEND: &str = "rodio";

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AudioError {
    #[error(
        "nessun dispositivo audio di uscita: collega cuffie o altoparlanti, \
         oppure attivane uno nelle impostazioni audio del sistema"
    )]
    NoDevice,
    #[error("librespot è stato compilato senza backend audio")]
    NoBackend,
    #[error("impossibile aprire il dispositivo audio: {0}")]
    Open(String),
}

/// The output chosen for playback: librespot's default backend on the system's
/// default device.
pub struct AudioOutput {
    backend: &'static str,
    builder: SinkBuilder,
    device: Option<String>,
}

impl AudioOutput {
    /// Picks librespot's default backend and checks that the system has a default
    /// output device. Nothing is opened yet: see [`AudioOutput::into_sink_builder`].
    pub fn system_default() -> Result<Self, AudioError> {
        let &(backend, builder) = audio_backend::BACKENDS.first().ok_or(AudioError::NoBackend)?;
        let device = if backend == RODIO_BACKEND {
            Some(default_device_name()?)
        } else {
            None
        };
        Ok(Self {
            backend,
            builder,
            device,
        })
    }

    /// Name of the librespot backend, for instance `rodio`.
    pub fn backend(&self) -> &str {
        self.backend
    }

    /// Name of the output device, when the backend says which one it uses.
    pub fn device_name(&self) -> Option<&str> {
        self.device.as_deref()
    }

    /// Sink builder for librespot's `Player`, which calls it on its own thread
    /// because an audio stream cannot move between threads. The receiver gets
    /// `Ok(())` once the device is open, or the reason it could not be opened.
    pub fn into_sink_builder(
        self,
    ) -> (
        impl FnOnce() -> Box<dyn Sink> + Send + 'static,
        mpsc::Receiver<Result<(), AudioError>>,
    ) {
        let (opened, receiver) = mpsc::sync_channel(1);
        let builder = self.builder;
        let build = move || match open_without_panic(builder) {
            Ok(sink) => {
                let _ = opened.send(Ok(()));
                sink
            }
            Err(err) => {
                let _ = opened.send(Err(err.clone()));
                Box::new(NoOutput(err)) as Box<dyn Sink>
            }
        };
        (build, receiver)
    }
}

/// Name of the device the rodio backend will pick, if there is one that works.
fn default_device_name() -> Result<String, AudioError> {
    let device = rodio::cpal::default_host()
        .default_output_device()
        .ok_or(AudioError::NoDevice)?;
    // A device that cannot report a format cannot play either; the backend unwraps this.
    device
        .default_output_config()
        .map_err(|err| AudioError::Open(err.to_string()))?;
    Ok(device.name().unwrap_or_else(|_| "dispositivo predefinito".to_string()))
}

thread_local! {
    /// Set while this thread opens the backend, whose panic is reported as an error.
    static OPENING: Cell<bool> = const { Cell::new(false) };
}

/// Calls the backend and turns its panic into an error, keeping the panic message
/// off stderr. The panic hook is replaced once; it defers to the previous hook on
/// every thread that is not opening the backend.
fn open_without_panic(builder: SinkBuilder) -> Result<Box<dyn Sink>, AudioError> {
    static QUIET_HOOK: Once = Once::new();
    QUIET_HOOK.call_once(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            if !OPENING.with(Cell::get) {
                previous(info);
            }
        }));
    });
    OPENING.with(|opening| opening.set(true));
    let result = panic::catch_unwind(AssertUnwindSafe(|| builder(None, AudioFormat::default())));
    OPENING.with(|opening| opening.set(false));
    result.map_err(|payload| AudioError::Open(panic_message(payload.as_ref())))
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    let message = payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .unwrap_or("errore sconosciuto");
    message
        .strip_prefix("called `Result::unwrap()` on an `Err` value: ")
        .unwrap_or(message)
        .to_string()
}

/// Stands in for a device that failed to open: librespot pauses on its errors.
/// The player drops it as soon as it learns that the device did not open.
struct NoOutput(AudioError);

impl Sink for NoOutput {
    fn start(&mut self) -> SinkResult<()> {
        Err(SinkError::NotConnected(self.0.to_string()))
    }

    fn write(&mut self, _packet: AudioPacket, _converter: &mut Converter) -> SinkResult<()> {
        Err(SinkError::NotConnected(self.0.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fails the way librespot's rodio backend does when there is no device.
    fn panicking_backend(_device: Option<String>, _format: AudioFormat) -> Box<dyn Sink> {
        panic!("called `Result::unwrap()` on an `Err` value: NoDeviceAvailable")
    }

    fn working_backend(_device: Option<String>, _format: AudioFormat) -> Box<dyn Sink> {
        Box::new(NoOutput(AudioError::NoDevice))
    }

    #[test]
    fn a_panicking_backend_becomes_a_readable_error() {
        let err = open_without_panic(panicking_backend).err().expect("the panic must become an error");
        assert_eq!(err, AudioError::Open("NoDeviceAvailable".to_string()));
        assert!(!OPENING.with(Cell::get), "the flag must be cleared after the panic");
    }

    #[test]
    fn a_working_backend_is_returned() {
        assert!(open_without_panic(working_backend).is_ok());
    }

    #[test]
    fn the_sink_builder_reports_the_failure_on_the_player_thread() {
        let output = AudioOutput {
            backend: "test",
            builder: panicking_backend,
            device: None,
        };
        let (build, opened) = output.into_sink_builder();
        let refused = std::thread::spawn(move || build().start().is_err())
            .join()
            .expect("the panic must not reach the thread");
        assert!(refused, "the stand-in sink must refuse to play");
        assert_eq!(opened.recv(), Ok(Err(AudioError::Open("NoDeviceAvailable".to_string()))));
    }

    #[test]
    fn the_messages_are_readable() {
        assert!(AudioError::NoDevice.to_string().starts_with("nessun dispositivo audio di uscita"));
        assert_eq!(
            AudioError::Open("x".to_string()).to_string(),
            "impossibile aprire il dispositivo audio: x"
        );
    }
}
