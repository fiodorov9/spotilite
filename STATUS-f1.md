# STATUS istanza f1 (F1: dipendenze e login)

Brief: `D:\spotilite\briefs\f1-login.md`. Ramo: `claude/f1`. Worktree: `D:\spotilite\worktrees\f1`.

| Task | Stato | Nota |
|------|-------|------|
| F1-1 dipendenze | FATTO | `cargo build` exit 0; `reqwest` in più rispetto al brief; `vergen` bloccato a 9.0.6 in `Cargo.lock` |
| F1-2 test auth (RED) | IN CORSO | |
| F1-3 config e auth (GREEN) | DA FARE | |
| F1-4 comando login | DA FARE | |

## Log

- Avvio: nessun `STATUS-f1.md`, nessun `RISPOSTE-*.md`, nessun commit F1 sul ramo (ultimo commit `840885e`, fine F0). Si parte da F1-1.
- F1-1, verifica dei feature flag (`cargo add --dry-run` e `cargo info librespot@0.8.0`): librespot 0.8.0 **non ha** feature chiamate `oauth` o `playback`. I sotto-crate `librespot-oauth` e `librespot-playback` sono dipendenze sempre attive; le feature sono solo TLS (`native-tls` di default), backend audio (`rodio-backend` di default) e discovery (`with-libmdns` di default). Quindi `librespot = "0.8"` con le feature di default copre oauth, playback e il backend audio predefinito.
- F1-1, scelte di versione allineate all'albero di librespot (nessuna crate duplicata): `rodio` 0.21 con `default-features = false` e sola feature `playback` (librespot la usa così, la decodifica la fa librespot); `rand` 0.9 e `base64` 0.22 (stesse di `librespot-core`); `sha2` 0.10 (stessa di `oauth2`); `dirs` 7.0.0 (ultima); `tokio` 1 con `rt-multi-thread`, `macros`, `signal`; `serde` 1 con `derive`; `serde_json` 1; `thiserror` 2.
- F1-1, dipendenza **in più rispetto all'elenco del brief: `reqwest` 0.12** (`default-features = false`, `native-tls`). Serve a F1-4 per lo scambio codice -> token (POST a `accounts.spotify.com/api/token`): nessuna crate dell'elenco fa richieste HTTP, e l'`OAuthClient` di `librespot-oauth` genera da sé PKCE e URL, quindi non può usare `authorize_url`/`parse_callback` come chiede il brief. `reqwest` 0.12 con `native-tls` è già nell'albero (lo usa `librespot-oauth`), quindi costo di compilazione nullo. Va deciso adesso perché `Cargo.toml` si tocca una volta sola.
- F1-1, prima `cargo build`: **fallita** (exit 101 dopo 185 s, log `D:\spotilite\build\f1-build1.log`) nello script di build di `librespot-core` 0.8.0: `the trait bound vergen::feature::build::Build: vergen_lib::entries::Add is not satisfied`. Causa: `vergen-gitcl` 1.0.8 chiede `vergen ^9.0.6` e `vergen-lib ^0.1.6`, ma il resolver ha scelto `vergen` 9.1.0, che usa `vergen-lib` 9.1.0 (rottura di semver in una minor). Non è un problema della macchina. Correzione: `cargo update -p vergen --precise 9.0.6`, che tocca **solo `Cargo.lock`** (versionato), non `Cargo.toml`.
- F1-1, seconda `cargo build`: `Finished dev profile [unoptimized + debuginfo] target(s) in 1m 18s`, exit 0, nessun warning (log `D:\spotilite\build\f1-build2.log`). Tempo totale della prima compilazione di librespot: circa 4,5 minuti (185 s + 78 s). `cargo tree -d`: i duplicati presenti (`base64` 0.23, `rand` 0.8, `thiserror` 1, `getrandom` 0.2/0.3/0.4, ...) vengono dalle dipendenze transitive di librespot; le versioni scelte qui coincidono con quelle usate direttamente da librespot.
