# spotilite

Client Spotify per desktop, leggero e scritto da zero in Rust.
Obiettivo: meno di 250 MB di RAM, su Windows e Linux.

## Requisiti

- Account Spotify **Premium** (gli account gratuiti non sono supportati).
- Rust **1.98** con `cargo`; su Windows serve la toolchain MSVC.

## Compilazione

```
cargo build --release
```

Il binario finisce in `target/release/` (oppure nella cartella indicata da `CARGO_TARGET_DIR`).

## Esecuzione

```
spotilite --help      # elenco dei comandi
spotilite --version   # nome e versione
spotilite login       # accesso all'account (non ancora implementato)
spotilite play        # riproduzione (non ancora implementato)
spotilite gui         # interfaccia grafica (non ancora implementato)
```

## Stato del progetto

Fase **F0** (fondamenta): esiste solo lo scheletro della riga di comando.
Accesso, riproduzione e interfaccia grafica arrivano nelle fasi successive.
L'architettura prevista è descritta in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Nota

spotilite è software indipendente: non è affiliato, approvato o sponsorizzato da Spotify.
Spotify è un marchio registrato di Spotify AB.

Licenza: MIT, vedi [LICENSE](LICENSE).
