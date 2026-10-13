# Riproduzione: interfaccia per la GUI (F4)

Quattro moduli della libreria `spotilite`; i tipi di librespot non escono da `player` e `audio`.

## `state`: logica pura, senza rete né audio
- `PlaybackState::new() -> PlaybackState` (anche `Default`): `Stopped`, coda vuota, volume 100.
- Comandi, tutti `-> Result<PlaybackEffect, StateError>`: `enqueue(&mut self, uri: impl Into<String>)`, `play(&mut self)`, `pause(&mut self)`, `stop(&mut self)`, `next(&mut self)`, `prev(&mut self)`, `set_volume(&mut self, volume: u8)`.
- Letture: `status(&self) -> Status` (`Stopped`, `Playing`, `Paused`), `queue(&self) -> &[String]`, `current(&self) -> Option<&str>`, `current_index(&self) -> Option<usize>`, `volume(&self) -> u8`.
- `PlaybackEffect`: `None`, `StartTrack(String)`, `Resume`, `Pause`, `Stop`, `SetVolume(u8)`. `StateError`: `EmptyQueue`, `EmptyUri`.

## `auth`: token fresco prima della sessione
- `async fn auth::fresh_token(store: &TokenStore, token: Token, token_endpoint: &str, client_id: &str) -> Result<Token, AuthError>`: restituisce il token così com'è se vale ancora per più di `REFRESH_MARGIN_SECS` (300 s), altrimenti lo rinnova con il refresh token e lo salva (scrittura atomica) prima di restituirlo. `token_endpoint`: `auth::TOKEN_ENDPOINT`; `client_id`: quello usato per l'accesso (di solito quello di librespot).
- Errori del rinnovo: `SessionExpired` («sessione scaduta: esegui `spotilite login`», refresh token revocato), `NotRenewable` (token scaduto senza refresh token), `RefreshNetwork`, `RefreshRefused`. In tutti i casi il file del token resta com'era.
- `async fn auth::refresh(token_endpoint, client_id, token: &Token) -> Result<Token, AuthError>`: solo il rinnovo, senza salvare.

## `player`: sessione e player di librespot
- `async fn Player::connect(token: &auth::Token) -> Result<Player, PlayerError>`: controlla la scadenza del token e il dispositivo audio, poi apre la sessione. Va chiamata dentro un runtime tokio, con il token di `auth::fresh_token`. `Player::connect_via(token, proxy: Option<&str>)` fa lo stesso passando da un proxy HTTP.
- `load(&self, uri: &str) -> Result<(), PlayerError>`, `play(&self)`, `pause(&self)`, `stop(&self)`, `set_volume(&self, volume: u8)`, `position(&self) -> Duration`, `username(&self) -> String`.
- `apply(&self, effect: &PlaybackEffect) -> Result<(), PlayerError>`: esegue l'esito della macchina a stati.
- `preload(&self, uri: &str) -> Result<(), PlayerError>`: scarica in anticipo un brano o un episodio, così il `load` successivo dello stesso URI parte subito; non fa nulla se è già caricato o in precaricamento.
- `async fn next_event(&mut self) -> Option<PlaybackEvent>`: `Loading`, `TrackInfo { title, artists, duration }`, `Playing { position }`, `Paused { position }`, `TimeToPreload` (il brano finisce tra circa 30 s ed è tutto scaricato), `EndOfTrack`, `Unavailable`, `Stopped`, tutti con `uri` (quello del brano a cui si riferiscono). `None`: il player di librespot si è fermato.
- `track_to_preload<'a>(event: &PlaybackEvent, state: &'a PlaybackState) -> Option<&'a str>`: dopo `TimeToPreload` del brano corrente, il brano seguente della coda; `None` per gli altri eventi, per un annuncio su un altro brano, da fermo e sull'ultimo brano. Logica pura, senza rete.
- `check_uri(uri: &str) -> Result<(), PlayerError>`: controllo senza rete di ciò che `load` e `preload` accettano: solo `spotify:track:` e `spotify:episode:`.
- `parse_request(uri: &str) -> Result<PlayRequest, PlayerError>`: controllo senza rete di ciò che si può chiedere di ascoltare. `PlayRequest::Item(String)` (brano o episodio) o `PlayRequest::Album(String)`, con l'URI in forma canonica, la stessa degli eventi. Le playlist danno `PlaylistNotYet` («arriveranno in una prossima versione»), artisti e podcast `NotPlayable`.
- `async fn album_tracks(&self, uri: &str) -> Result<Vec<String>, PlayerError>`: i brani dell'album in ordine d'ascolto (disco 1, poi disco 2, ...), letti dai metadati di Spotify, da accodare uno per uno con `state.enqueue`. Errore: `Album { uri, reason }`.
- `PlayerError`: `TokenExpired`, `Rejected`, `Connect`, `Audio(AudioError)`, `InvalidUri`, `NotPlayable`, `PlaylistNotYet`, `AlbumNotLoadable` (album passato a `load`), `Album`, `Internal`; messaggi in italiano.

## `audio`: uscita audio
- `AudioOutput::system_default() -> Result<AudioOutput, AudioError>`, poi `backend(&self) -> &str`, `device_name(&self) -> Option<&str>`. `Player::connect` la usa da sé.
- `AudioError`: `NoDevice`, `NoBackend`, `Open(String)`. Senza dispositivo si ottiene un errore, mai un panico.

## Ciclo tipico
1. `let effect = state.enqueue(uri)?; player.apply(&effect)?;` e lo stesso per play, pausa, next, prev, volume. Per un album: `for uri in player.album_tracks(&album).await? { player.apply(&state.enqueue(uri)?)?; }`.
2. Su `PlaybackEvent::TimeToPreload`: `if let Some(next) = player::track_to_preload(&event, &state) { player.preload(next)?; }`, così il brano seguente parte senza pausa.
3. Su `PlaybackEvent::EndOfTrack` o `Unavailable`: `player.apply(&state.next()?)?` (se il brano seguente è stato precaricato, librespot usa i dati già scaricati).
4. La barra di avanzamento legge `player.position()`, che avanza da sola mentre il brano suona.

Note: gli eventi di un `load` superato vengono scartati; `Player` è `Send`; i comandi non bloccano, il `Drop` ferma la riproduzione e attende il thread di librespot.
