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
spotilite login       # accesso all'account: apre il browser (--print-url stampa solo l'indirizzo)
spotilite auth status # dice se c'è un token salvato e quando scade, senza mostrarlo
spotilite play <uri>  # riproduce in ordine brani o episodi (spotify:track:...), Ctrl+C per fermare
spotilite gui         # interfaccia grafica (non ancora implementato)
```

## Stato del progetto

Fase **F1** (dipendenze e accesso): `spotilite login` usa OAuth con PKCE e salva il token
in `%APPDATA%\spotilite\token.json` su Windows, `~/.local/state/spotilite/token.json` su Linux.
Fase **F2** (riproduzione): `spotilite play` suona brani ed episodi con librespot, sul dispositivo
audio predefinito; album e playlist non sono ancora supportati.
Fase **F3** (sessione continua): il token di accesso dura un'ora e `spotilite play` lo rinnova
da sé quando è scaduto o sta per scadere; `spotilite login` serve solo se Spotify revoca l'accesso.
L'interfaccia grafica arriva nella fase successiva.
L'architettura prevista è descritta in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Nota

spotilite è software indipendente: non è affiliato, approvato o sponsorizzato da Spotify.
Spotify è un marchio registrato di Spotify AB.

Licenza: MIT, vedi [LICENSE](LICENSE).
