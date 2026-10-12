# STATUS istanza f2 (F2: riproduzione audio)

Brief: `D:\spotilite\briefs\f2-riproduzione.md`. Ramo: `claude/f2`. Worktree: `D:\spotilite\worktrees\f2`.

| Task | Stato | Nota |
|------|-------|------|
| F2-1 test macchina a stati (RED) | FATTO | `cargo test --test state`: 0 passed, 22 failed (`not yet implemented`), exit 101 |
| F2-2 macchina a stati (GREEN) | FATTO | `cargo test --test state`: 22 passed; `cargo test`: 45 passed; clippy pulito |
| F2-3 player e audio su librespot | FATTO | `cargo test`: 56 passed; dispositivo reale di questa macchina aperto con il backend di librespot; `docs/PLAYER.md` 28 righe |
| F2-4 comando `play` | IN CORSO | |
| F2-5 verifica e rapporto | DA FARE | |

## Log

- Avvio: nessun `STATUS-f2.md`, nessun `RISPOSTE-*.md`, nessun commit F2 sul ramo, che parte da `4780ecd` (merge di F1 in `main`). Si parte da F2-1. Variabili d'ambiente come da brief (`CARGO_TARGET_DIR=D:\spotilite\build\target\f2`, cargo e rustc 1.98.0). La cartella target di f2 era vuota: prima compilazione completa avviata in background (log `D:\spotilite\build\f2-build1.log`).
- Prima compilazione (`cargo build --all-targets`, cartella target vuota): `Finished dev profile ... in 6m 47s`, exit 0, nessun warning.
- F2-1: `src/state.rs` contiene già tipi e firme (`PlaybackState`, `Status`, `PlaybackEffect`, `StateError`), con corpi `todo!()`: così il RED è un fallimento dei test e non un errore di compilazione. `tests/state.rs` ha 22 test: i 6 punti del brief più i casi limite. Semantica scelta, scritta nei test:
  - la coda ha un cursore; da `Stopped` il cursore indica il brano che `play()` farebbe partire. `enqueue` su `Stopped` parte dal cursore: a coda vuota è il primo brano (punto 2 del brief), a coda finita è il brano appena aggiunto;
  - `next()` sull'ultimo brano dà `Stop` e lascia la coda intatta (serve a `prev()`); altri `next()` danno `None`;
  - `prev()` sul primo brano riparte da capo (`StartTrack` del primo brano, come fanno i lettori musicali); dopo la fine coda torna all'ultimo brano;
  - `next()`/`prev()` da `Paused` fanno partire il brano; `play()` dopo la fine coda riparte dal primo brano;
  - `Ok(None)` quando il comando è già soddisfatto (idempotenza: `pause()` su `Stopped`, `play()` su `Playing`); `Err` solo se il comando non si può eseguire (`EmptyQueue` per `play`/`next`/`prev` a coda vuota, `EmptyUri` per un URI vuoto o di soli spazi);
  - volume `u8` tagliato a 100 (`set_volume(150)` e `set_volume(255)` danno 100), volume iniziale 100 (nessuna attenuazione: comanda il volume di sistema);
  - in più `stop()`: tiene il brano corrente, e `play()` lo fa ripartire da capo.
- F2-1, RED: `cargo test --test state` -> `test result: FAILED. 0 passed; 22 failed`, exit 101, tutti con `not yet implemented: F2-2`. Nessun warning di compilazione.
- F2-2: `PlaybackState` tiene `queue`, `cursor`, `status`, `volume`. Ogni comando restituisce `Result<PlaybackEffect, StateError>` con `PlaybackEffect` = `None`, `StartTrack(uri)`, `Resume`, `Pause`, `Stop`, `SetVolume(u8)`. Un solo punto (`start`) sposta il cursore e accede alla coda, con `get` e non con l'indice: nessun panico possibile. URI salvati senza spazi iniziali e finali.
- F2-2, verifica: `cargo test --test state` -> `test result: ok. 22 passed; 0 failed`; `cargo test` (tutta la suite) -> main 1, auth 11, cli 2, login 9, state 22: **45 passed, 0 failed**. `cargo clippy --all-targets`: un solo avviso, `should_implement_trait` su `next()` (il nome lo impone il brief e lo stato non è un iteratore): soppresso con `#[expect(..., reason = ...)]`, poi exit 0 senza warning.
- F2-3, scoperta che ha guidato `audio.rs`: il backend predefinito di librespot 0.8 (`rodio`) apre il dispositivo con `create_sink(...).unwrap()`, quindi **senza dispositivo va in panico**, e lo fa nel thread del player di librespot, perché `Player::new` chiama il costruttore del sink su quel thread (lo stream audio non è `Send`). Difesa su due livelli:
  1. `AudioOutput::system_default()` prende il primo backend di `audio_backend::BACKENDS` (rodio) e prima di qualunque rete controlla con `rodio::cpal` (riesportato da `rodio`, stessa `cpal` 0.16 di librespot) che esista un dispositivo predefinito con un formato valido: se manca, `AudioError::NoDevice` con un messaggio in italiano;
  2. il costruttore del sink passato a librespot gira dentro `catch_unwind`; un panic hook installato una volta tace solo sul thread che sta aprendo il dispositivo (flag `thread_local`) e negli altri casi chiama quello precedente. L'esito (aperto o errore) torna a `Player::connect` su un canale; in caso di errore il player di librespot riceve un sink sostitutivo che rifiuta di suonare, e `connect` lo chiude e restituisce `PlayerError::Audio`.
- F2-3, `player.rs`: `Player` incapsula `Session`, il `Player` di librespot e un `SoftMixer`. `connect(token)` rifiuta un token scaduto (`TokenExpired`), controlla l'audio, poi `Session::new(SessionConfig::default(), None)` + `Credentials::with_access_token` (il token OAuth di F1, ottenuto con il client ID di librespot: nessun nome utente o password). Qualità 320 kbit/s (`Bitrate320`, serve Premium che è comunque richiesto); volume iniziale 100 come la macchina a stati. Errori di accesso (`PermissionDenied`/`Unauthenticated`) -> `Rejected` con l'invito a rifare il login con un account Premium.
- F2-3, eventi e posizione: un task tokio legge gli eventi di librespot, li traduce in `PlaybackEvent` (con URI, titolo, artisti, durata) e aggiorna un orologio: librespot comunica la posizione solo a inizio, pausa e salti, quindi `position()` la calcola dall'ultimo valore più il tempo trascorso mentre suona. Gli eventi di un `load` superato vengono scartati (conteggio dei `PlayRequestIdChanged` con ID diverso, perché un seek durante il caricamento riannuncia lo stesso ID): così un `EndOfTrack` in ritardo non fa saltare il brano che lo ha sostituito.
- F2-3, `tokio::sync::mpsc` viene da `tokio` con le feature `sync` e `time`, attivate da `librespot-core` (unificazione delle feature di Cargo): nessuna modifica a `Cargo.toml`. Stessa cosa per `rodio::cpal`.
- F2-3, unit test (11, senza rete né audio): panico del backend convertito in errore leggibile e flag ripulito; il costruttore del sink, chiamato su un altro thread, segnala il fallimento e il sink sostitutivo rifiuta `start()`; messaggi; URI riproducibili (brano, episodio) e rifiutati (album con motivo, `spotify:track:xxx`, link https, stringa vuota); volume 0..100 -> 0..65535; orologio; filtro degli eventi superati; traduzione degli eventi; `Player` è `Send`. Con `--nocapture` il panico simulato non stampa nulla su stderr.
- F2-3, **prova reale su questa macchina** (esempio temporaneo, poi cancellato): `AudioOutput::system_default()` -> backend `rodio`, dispositivo `Altoparlanti (Realtek(R) Audio)`; il costruttore del sink chiamato su un thread separato apre il dispositivo e `start()` riesce: `apertura: Ok(Ok(()))`. Il ramo «nessun dispositivo» non è provabile qui (il dispositivo c'è): è coperto dagli unit test.
- F2-3, documentazione: `docs/PLAYER.md` (28 righe) con le firme esatte per F3; in `docs/ARCHITECTURE.md` aggiornati i moduli presenti e la direzione `cli`/`ui` -> `state` -> esito -> `player`.
- F2-3, verifica: `cargo clippy --all-targets` exit 0 senza warning; `cargo test` -> lib 11, main 1, auth 11, cli 2, login 9, state 22: **56 passed, 0 failed**.
