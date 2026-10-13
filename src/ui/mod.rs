//! Graphical interface: the window, its view model and the engine thread that plays.
//!
//! The window never touches the network or the audio device. It sends
//! [`engine::Command`]s to the engine thread and draws what the
//! [`viewmodel::ViewModel`] makes of the [`engine::EngineEvent`]s that come back.

pub mod engine;
pub mod viewmodel;
