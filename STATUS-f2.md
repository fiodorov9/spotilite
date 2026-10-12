# STATUS istanza f2 (F2: riproduzione audio)

Brief: `D:\spotilite\briefs\f2-riproduzione.md`. Ramo: `claude/f2`. Worktree: `D:\spotilite\worktrees\f2`.

| Task | Stato | Nota |
|------|-------|------|
| F2-1 test macchina a stati (RED) | FATTO | `cargo test --test state`: 0 passed, 22 failed (`not yet implemented`), exit 101 |
| F2-2 macchina a stati (GREEN) | IN CORSO | |
| F2-3 player e audio su librespot | DA FARE | |
| F2-4 comando `play` | DA FARE | |
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
