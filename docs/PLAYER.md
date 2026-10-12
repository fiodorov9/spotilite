# Riproduzione: interfaccia per la GUI (F3)

Tre moduli della libreria `spotilite`; i tipi di librespot non escono da `player` e `audio`.

## `state`: logica pura, senza rete né audio
- `PlaybackState::new() -> PlaybackState` (anche `Default`): `Stopped`, coda vuota, volume 100.
- Comandi, tutti `-> Result<PlaybackEffect, StateError>`: `enqueue(&mut self, uri: impl Into<String>)`, `play(&mut self)`, `pause(&mut self)`, `stop(&mut self)`, `next(&mut self)`, `prev(&mut self)`, `set_volume(&mut self, volume: u8)`.
- Letture: `status(&self) -> Status` (`Stopped`, `Playing`, `Paused`), `queue(&self) -> &[String]`, `current(&self) -> Option<&str>`, `current_index(&self) -> Option<usize>`, `volume(&self) -> u8`.
- `PlaybackEffect`: `None`, `StartTrack(String)`, `Resume`, `Pause`, `Stop`, `SetVolume(u8)`. `StateError`: `EmptyQueue`, `EmptyUri`.

## `player`: sessione e player di librespot
- `async fn Player::connect(token: &auth::Token) -> Result<Player, PlayerError>`: controlla la scadenza del token e il dispositivo audio, poi apre la sessione. Va chiamata dentro un runtime tokio.
- `load(&self, uri: &str) -> Result<(), PlayerError>`, `play(&self)`, `pause(&self)`, `stop(&self)`, `set_volume(&self, volume: u8)`, `position(&self) -> Duration`, `username(&self) -> String`.
- `apply(&self, effect: &PlaybackEffect) -> Result<(), PlayerError>`: esegue l'esito della macchina a stati.
- `async fn next_event(&mut self) -> Option<PlaybackEvent>`: `Loading`, `TrackInfo { title, artists, duration }`, `Playing { position }`, `Paused { position }`, `EndOfTrack`, `Unavailable`, `Stopped`, tutti con `uri`. `None`: il player di librespot si è fermato.
- `check_uri(uri: &str) -> Result<(), PlayerError>`: controllo senza rete; si riproducono solo `spotify:track:` e `spotify:episode:`.
- `PlayerError`: `TokenExpired`, `Rejected`, `Connect`, `Audio(AudioError)`, `InvalidUri`, `NotPlayable`, `Internal`; messaggi in italiano.

## `audio`: uscita audio
- `AudioOutput::system_default() -> Result<AudioOutput, AudioError>`, poi `backend(&self) -> &str`, `device_name(&self) -> Option<&str>`. `Player::connect` la usa da sé.
- `AudioError`: `NoDevice`, `NoBackend`, `Open(String)`. Senza dispositivo si ottiene un errore, mai un panico.

## Ciclo tipico
1. `let effect = state.enqueue(uri)?; player.apply(&effect)?;` e lo stesso per play, pausa, next, prev, volume.
2. Su `PlaybackEvent::EndOfTrack` o `Unavailable`: `player.apply(&state.next()?)?`.
3. La barra di avanzamento legge `player.position()`, che avanza da sola mentre il brano suona.

Note: gli eventi di un `load` superato vengono scartati; `Player` è `Send`; i comandi non bloccano, il `Drop` ferma la riproduzione e attende il thread di librespot.
