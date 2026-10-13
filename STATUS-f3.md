# STATUS istanza f3 (F3: sessione continua e album)

Brief: `D:\spotilite\briefs\f3-sessione.md`. Ramo: `claude/f3`. Worktree: `D:\spotilite\worktrees\f3`.

| Task | Stato | Nota |
|------|-------|------|
| F3-1 test del rinnovo automatico (RED) | FATTO | `cargo test --test refresh`: 3 passed, 6 failed (nessun rinnovo all'endpoint finto), exit 101 |
| F3-2 rinnovo in `auth.rs` (GREEN) | IN CORSO | |
| F3-3 precaricamento del brano successivo | DA FARE | |
| F3-4 play di un album intero | DA FARE | |
| F3-5 verifica e rapporto | DA FARE | |

## Log

- Avvio: nessun `STATUS-f3.md`, nessun `RISPOSTE-*.md`, nessun commit F3 sul ramo, che parte da `0cb9a28` (merge di F2 in `main`). Si parte da F3-1. Variabili d'ambiente come da brief (`CARGO_TARGET_DIR=D:\spotilite\build\target\f3`, cargo e rustc 1.98.0). La cartella target di f3 non esisteva: prima compilazione completa avviata in background (log `D:\spotilite\build\f3-build1.log`).
- Letti per intero `docs/PLAYER.md`, `src/auth.rs`, `src/player.rs`, `src/main.rs` e i test `login.rs`, `play.rs`, `auth.rs`. Verificato nel sorgente di librespot 0.8: `PlayerEvent::TimeToPreloadNextTrack { play_request_id, track_id }` porta l'URI del brano **in corso** (scatta quando ne mancano 30 s ed è tutto scaricato), non quello successivo: il brano da precaricare lo sceglie la coda di spotilite. `Player::preload` su un brano già caricato o già in precaricamento non fa nulla; un `load` dello stesso brano usa i dati precaricati.
- Prima compilazione (`cargo build --all-targets`, cartella target vuota): `Finished dev profile ... in 9m 22s`, exit 0, nessun warning.
- F3-1, agganci aggiunti a `play` nello stile di `login`:
  - `--token-endpoint` (nascosto, come in `login`): l'endpoint dei token, che i test sostituiscono con quello finto;
  - `--proxy` (nascosto): proxy HTTP della sessione Spotify. Serve perché con un token valido (casi 1, 2 e 4 del brief) `play` va avanti fino a collegarsi ai **veri** server di Spotify: i test gli danno un proxy finto locale che chiude subito ogni connessione, così la sessione fallisce («impossibile collegarsi a Spotify», exit 1) senza uscire dalla macchina. Il proxy finto conta le connessioni: è la prova che la sessione è partita. Con una porta chiusa invece del proxy finto, su Windows librespot impiega 36 s (12 tentativi rifiutati); con il proxy finto l'intero file di test gira in circa 1 s. In `player.rs` il proxy passa da `Player::connect_via(token, proxy)`; `Player::connect(token)` resta e chiama `connect_via(token, None)`;
  - `--client-id` (visibile, come in `login`): il rinnovo deve usare lo stesso client ID dell'accesso, e il file del token non lo salva (aggiungere un campo a `Token` avrebbe rotto i test esistenti che lo costruiscono). Predefinito: quello di librespot, come in `login`.
  - `auth status` non fa chiamate di rete (resta informativo), quindi non ha bisogno di flag nuovi.
- F3-1, in `src/auth.rs` solo il punto d'aggancio `fresh_token(store, token, endpoint, client_id)` con la firma definitiva, che per ora restituisce il token così com'è (comportamento di F2): così il RED è un fallimento dei test e non un errore di compilazione, e i flag non restano inutilizzati. `play` lo chiama dopo il controllo degli URI (un errore di battitura si segnala prima di qualunque rete) e prima di `Player::connect_via`.
- F3-1, `tests/refresh.rs`, 9 test end-to-end sul binario (endpoint finto e proxy finto locali, `APPDATA` reindirizzato, nessun segreto in output):
  1. token scaduto: un solo POST all'endpoint con `grant_type=refresh_token`, `refresh_token=rt-old`, `client_id=`; nuovo access token, scadenza e scope salvati; nessun file temporaneo; `play` va avanti a collegarsi (stdout «Collegamento a Spotify», nessun «scadut» su stderr, connessioni al proxy finto > 0 oppure errore del dispositivo audio su una macchina senza);
  2. refresh token vecchio tenuto se la risposta non ne ha uno; sostituito se la risposta ne porta uno nuovo (due test);
  3. `invalid_grant`: stderr «sessione scaduta» e «spotilite login», exit 1, file del token identico byte per byte, nessun file temporaneo, nessuna sessione aperta;
  4. token valido per un'ora: l'endpoint finto non riceve nulla, file identico, `play` va avanti a collegarsi;
  5. token assente: «nessun token salvato: esegui prima `spotilite login`», endpoint e proxy non ricevono nulla;
  - in più: token che scade tra 60 s («in scadenza») rinnovato; errore di rete durante il rinnovo (endpoint chiuso) segnalato con file intatto; `auth status` su token scaduto dice «scaduto il 1970-01-12 ...» ed esce 0 senza toccare il file.
- F3-1, `tests/play.rs`: il test di F2 `play_with_an_expired_token_asks_to_sign_in_again` salvava un token scaduto **con** refresh token e non passava `--token-endpoint`: dopo F3-2 avrebbe chiamato l'endpoint vero di Spotify. Diventa `play_with_an_expired_token_that_cannot_be_renewed_asks_to_sign_in_again`: stesso token ma senza refresh token, stesse asserzioni («scaduto», «spotilite login», nessun segreto), nessuna rete.
- F3-1, RED: `cargo test --test refresh` -> `test result: FAILED. 3 passed; 6 failed`, exit 101; i 6 falliscono perché l'endpoint finto non riceve il rinnovo (`got: []`) o perché `play` risponde ancora «il token di accesso è scaduto». Passano già i casi 4, 5 e `auth status`. Resto della suite: lib 11, main 2, auth 11, cli 2, login 9, play 6, state 22, tutti ok. Compilazione senza warning.
