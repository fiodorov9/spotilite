# STATUS istanza i1 (F0: fondamenta)

Brief: `D:\spotilite\briefs\f0-fondamenta.md`. Ramo: `main`.

| Task | Stato | Nota |
|------|-------|------|
| F0-1 init repo e crate | FATTO | `git init -b main`, `cargo init --name spotilite --vcs none .`, identità locale impostata |
| F0-2 test CLI (RED) | FATTO | `tests/cli.rs` creato, `cargo test --test cli` fallisce come atteso (2 failed, exit 101) |
| F0-3 CLI con clap (GREEN) | FATTO | clap 4.6.7 (derive) + anyhow 1.0.104; `cargo test --test cli`: 2 passed |
| F0-4 file di progetto | DA FARE | |
| F0-5 verifica finale | DA FARE | |

## Log

- F0-1: repo vuota alla partenza (nessun commit, nessun STATUS/RISPOSTE precedente). Toolchain: cargo 1.98.0, rustc 1.98.0, git 2.55.0.windows.3. Il tool di shell disponibile in questa sessione è Git Bash, non PowerShell come diceva il brief: nessun impatto sui task.
- F0-2: il brief non indica un messaggio di commit per F0-2; per rispettare «un commit per task» ho usato `test(cli): add the command line tests`. Output RED: `help must mention 'login', got: Hello, world!` e `version output must name the app, got: Hello, world!`.
- F0-3: `login`, `play`, `gui` stampano `<comando>: non ancora implementato` ed escono con 0 (provato a mano). Senza argomenti stampa l'help ed esce con 0; un sottocomando sconosciuto esce con 2 (errore di clap). `cargo clippy --all-targets` senza warning. Help dei sottocomandi in inglese (sono doc comment nel codice).
