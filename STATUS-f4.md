# STATUS istanza f4 (F4: interfaccia grafica egui)

Brief: `D:\spotilite\briefs\f4-gui.md`. Ramo: `claude/f4`. Worktree: `D:\spotilite\worktrees\f4`.

| Task | Stato | Nota |
|------|-------|------|
| F4-1 test del modello di vista (RED) | FATTO | `cargo test --test ui_view`: 0 passed, 28 failed (`not yet implemented: F4-2`), exit 101; resto della suite 88 passed |
| F4-2 modello di vista e finestra (GREEN) | IN CORSO | |
| F4-3 controlli di riproduzione | DA FARE | |
| F4-4 coda, aggiunta URI, errori in-app | DA FARE | |
| F4-5 misure e verifica | DA FARE | |

## Log

- Avvio: nessun `STATUS-f4.md`, nessun `RISPOSTE-*.md`, nessun commit F4 sul ramo, che parte da `7806d3b` (merge di F3 in `main`). Si parte da F4-1. Variabili d'ambiente come da brief (`CARGO_TARGET_DIR=D:\spotilite\build\target\f4`, cargo e rustc 1.98.0). La cartella target di f4 non esisteva: prima compilazione completa avviata in background (log `D:\spotilite\build\f4-build1.log`).
- Letti `docs/ARCHITECTURE.md`, `docs/PLAYER.md`, `src/state.rs`, `src/player.rs`, `src/auth.rs`, `src/audio.rs`, `src/main.rs`, `STATUS-f3.md` (note per le fasi successive: la GUI deve rinnovare il token con lo stesso client ID di `login`).
- Versione di eframe: `cargo info eframe` -> 0.36.2, `rust-version: 1.95`, compatibile con Rust 1.98. In 0.36 il trait `eframe::App` ha `logic(ctx, frame)` e `ui(ui, frame)` (non più `update`).
- Prima compilazione (`cargo build --all-targets`, cartella target vuota): `Finished dev profile ... in 3m 50s`, exit 0, nessun warning.
- F4-1, struttura scelta (in `src/ui/`):
  - `engine.rs`: per ora solo i messaggi tra finestra e motore. `Command` (finestra -> motore): `TogglePlay` (il motore decide play o pausa sul proprio stato, così un evento in arrivo non crea corse), `Next`, `Prev`, `SetVolume(u8)`, `Add(PlayRequest)` (URI già riconosciuto da `player::parse_request`), `Reconnect`. `EngineEvent` (motore -> finestra): `Connecting`, `Connected { username }`, `Offline(Problem)`, `State(PlaybackState)` (copia della macchina a stati dopo ogni cambiamento), `Pending(Vec<PlayRequest>)` (richieste in attesa della sessione), `Playback(PlaybackEvent)`, `Position(Duration)`, `AlbumQueued { uri, tracks }`, `Failed(Problem)`. `Problem` classifica gli errori per ciò che l'utente può fare: `NotSignedIn`, `SignIn`, `Audio`, `Network`, `Uri`, `Other`;
  - `viewmodel.rs`: `ViewModel` (stato della finestra, alimentato da `apply(EngineEvent)`), `NowPlaying`, `QueueRow`, `Notice`/`Level`, `format_time`, `problem_text`. Nessuna dipendenza da egui: si prova senza finestra.
- F4-1, RED: tipi e firme con corpi `todo!("F4-2")`, come in F2-1, così il RED è un fallimento dei test e non un errore di compilazione. `tests/ui_view.rs`, 28 test puri (nessuna finestra, nessuna rete):
  - tempi `m:ss` (minuti oltre 59 per gli episodi lunghi);
  - vista iniziale: fermo, nessun brano, coda vuota, volume 100;
  - brano corrente: titolo, artisti separati da virgola, durata `m:ss`; prima di `TrackInfo` al posto del titolo c'è l'URI e la durata è `-:--`; stato «Caricamento...» dopo `Loading` fino a `Playing`; le informazioni di un altro brano si tengono per quando toccherà a lui; a coda finita nessun brano;
  - stato «Fermo», «In riproduzione», «In pausa» dalla macchina a stati; etichetta del pulsante «Play»/«Pausa»;
  - posizione da `Playing`/`Paused` del brano corrente e da `Position`; barra ferma a 1 oltre la fine e a 0 senza durata; eventi di un altro brano ignorati; brano nuovo da 0:00;
  - coda: numeri da 1, brano corrente evidenziato, «Titolo - Artisti» per i brani noti e l'URI per gli altri; nessuna riga evidenziata a coda finita; richieste in attesa della sessione dopo la coda, marcate (l'album dice «album»);
  - volume dalla macchina a stati; il cursore si muove subito e manda `SetVolume` (tagliato a 100);
  - URI scritti: brano, forma non canonica, album -> `Add` con l'URI canonico; anche i link `https://open.spotify.com/...` di «Copia link» (con `?si=` e `intl-it/`); URI non validi, playlist, artisti -> nessun comando e un avviso; testo vuoto ignorato;
  - sessione: «Collegamento a Spotify...», «Collegato come ...»; senza token invito a `spotilite login`; sessione scaduta: invito detto una volta sola, aggiunto se il messaggio non lo contiene; audio e rete non chiedono il login;
  - messaggi: frasi con l'iniziale maiuscola, senza nomi di varianti Rust né virgolette di debug; errori del motore come avviso d'errore senza perdere la sessione; brano non disponibile («Prima» non è disponibile); album accodato («12 brani», «1 brano»).
- F4-1, RED: `cargo test --test ui_view` -> `test result: FAILED. 0 passed; 28 failed`, exit 101, tutti `not yet implemented: F4-2`. Resto della suite: lib 24, main 3, auth 12, cli 2, login 9, play 7, refresh 9, state 22, tutti ok. `cargo clippy --all-targets`: 0 warning.
