# STATUS istanza i1 (F0: fondamenta)

Brief: `D:\spotilite\briefs\f0-fondamenta.md`. Ramo: `main`.

| Task | Stato | Nota |
|------|-------|------|
| F0-1 init repo e crate | FATTO | `git init -b main`, `cargo init --name spotilite --vcs none .`, identità locale impostata |
| F0-2 test CLI (RED) | FATTO | `tests/cli.rs` creato, `cargo test --test cli` fallisce come atteso (2 failed, exit 101) |
| F0-3 CLI con clap (GREEN) | FATTO | clap 4.6.7 (derive) + anyhow 1.0.104; `cargo test --test cli`: 2 passed |
| F0-4 file di progetto | FATTO | `.gitignore`, `LICENSE` (MIT), `README.md` (40 righe), `docs/ARCHITECTURE.md` (52 righe) |
| F0-5 verifica finale | FATTO | build release ok, `--version` e `--help` ok, `cargo test`: 2 passed, 0 failed |

## Log

- F0-1: repo vuota alla partenza (nessun commit, nessun STATUS/RISPOSTE precedente). Toolchain: cargo 1.98.0, rustc 1.98.0, git 2.55.0.windows.3. Il tool di shell disponibile in questa sessione è Git Bash, non PowerShell come diceva il brief: nessun impatto sui task.
- F0-2: il brief non indica un messaggio di commit per F0-2; per rispettare «un commit per task» ho usato `test(cli): add the command line tests`. Output RED: `help must mention 'login', got: Hello, world!` e `version output must name the app, got: Hello, world!`.
- F0-3: `login`, `play`, `gui` stampano `<comando>: non ancora implementato` ed escono con 0 (provato a mano). Senza argomenti stampa l'help ed esce con 0; un sottocomando sconosciuto esce con 2 (errore di clap). `cargo clippy --all-targets` senza warning. Help dei sottocomandi in inglese (sono doc comment nel codice).
- F0-4: `cargo search librespot` conferma che `librespot = "0.8.0"` esiste su crates.io (non aggiunto alle dipendenze, come da brief). Nessun em dash o en dash nei file del repo (controllato con grep).
- F0-5: `cargo build --release` finito in 11.03s (exit 0); `spotilite.exe --version` stampa `spotilite 0.1.0`; `--help` elenca `login`, `play`, `gui`; `cargo test` 0 unit test + 2 test di integrazione, tutti ok. `spotilite.exe` release pesa 631 KB. Commit di F0-5 (non indicato nel brief): `chore: record the F0 final check`.

## Note per le fasi successive

- I crate scaricati da `cargo add`/`cargo build` finiscono nella cache di `CARGO_HOME` (`D:\rust\.cargo\registry`), fuori da `D:\spotilite`: è il comportamento standard della toolchain indicata dal brief, nessun altro file scritto fuori.
- `Cargo.lock` è versionato (crate binario).
- Il modulo `cli` di `docs/ARCHITECTURE.md` per ora vive in `src/main.rs`, come chiedeva il brief: va estratto in `src/cli.rs` quando arrivano gli altri moduli.
