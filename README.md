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
spotilite play <uri>  # riproduce in ordine brani, episodi o album interi (spotify:track:..., spotify:album:...), Ctrl+C per fermare
spotilite gui         # apre la finestra (si possono passare degli URI da accodare: spotilite gui <uri>...)
```

## Interfaccia grafica

`spotilite gui` apre una finestra con il brano in ascolto, la barra di avanzamento, i
pulsanti precedente / play-pausa / successivo, il volume, la coda e un campo per aggiungere
brani, episodi e album. Prima serve un accesso salvato con `spotilite login`; il token si
rinnova da sé come per `play` (stesso `--client-id`, se all'accesso ne è stato usato uno diverso).

- **Aggiungere**: nel campo in basso si scrive o si incolla un URI (`spotify:track:...`,
  `spotify:episode:...`, `spotify:album:...`) oppure un link di «Copia link» di Spotify
  (`https://open.spotify.com/...`), poi Invio o «Aggiungi». Un album si accoda brano per brano.
  Gli URI passati a `spotilite gui <uri>...` si accodano all'apertura.
- **Tastiera**: la barra spaziatrice mette in pausa e riprende, tranne mentre si scrive nel campo.
- **Errori**: compaiono nella finestra. Senza accesso salvato (o con l'accesso scaduto) la
  finestra invita a eseguire `spotilite login` in un terminale e poi a premere «Riprova»;
  lo stesso pulsante serve dopo un problema di rete o del dispositivo audio. Quello che si
  aggiunge senza sessione resta in coda «in attesa della sessione» e parte appena c'è.
- **Grafica**: la finestra usa OpenGL; dove OpenGL 2.0 non c'è (schermi virtuali, alcune
  sessioni remote) passa da sola a wgpu: Direct3D 12 su Windows, la scelta predefinita di
  wgpu su Linux (di solito Vulkan). La variabile d'ambiente `WGPU_BACKEND` (per esempio
  `dx12`, `vulkan`) sceglie il backend di wgpu a mano.

Per i controlli automatici `spotilite gui --selftest [N]` (opzione nascosta) disegna N frame
(5 se manca il numero), senza collegarsi a Spotify né aprire il dispositivo audio, e si
chiude con exit 0; se la finestra non riesce a disegnarli entro 30 s (più 100 ms per frame)
esce comunque, con exit 1.

## Stato del progetto

Fase **F1** (dipendenze e accesso): `spotilite login` usa OAuth con PKCE e salva il token
in `%APPDATA%\spotilite\token.json` su Windows, `~/.local/state/spotilite/token.json` su Linux.
Fase **F2** (riproduzione): `spotilite play` suona brani ed episodi con librespot, sul dispositivo
audio predefinito.
Fase **F3** (sessione continua e album): il token di accesso dura un'ora e `spotilite play` lo rinnova
da sé quando è scaduto o sta per scadere; `spotilite login` serve solo se Spotify revoca l'accesso.
Negli ultimi 30 secondi di un brano si scarica in anticipo il successivo, per ridurre la pausa tra i due.
`spotilite play spotify:album:...` accoda tutti i brani dell'album, disco per disco; i brani non
disponibili si saltano con un avviso. Le playlist non sono ancora supportate: arriveranno in una
fase successiva, e per ora `play` le rifiuta con un messaggio che lo dice.
Fase **F4** (interfaccia grafica): `spotilite gui`, con egui/eframe; la riproduzione gira
in un thread separato, così la finestra non si blocca mai. Funzionale e leggera: la
rifinitura estetica arriverà dopo. Consumi misurati in [MISURE-f4.md](MISURE-f4.md).
L'architettura prevista è descritta in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Nota

spotilite è software indipendente: non è affiliato, approvato o sponsorizzato da Spotify.
Spotify è un marchio registrato di Spotify AB.

Licenza: MIT, vedi [LICENSE](LICENSE).
