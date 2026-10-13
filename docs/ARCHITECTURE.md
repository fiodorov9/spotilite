# Architettura di spotilite

Riferimento per le fasi dopo F0. Descrive i moduli previsti e le regole che li legano.
Oggi il codice contiene la riga di comando (`src/main.rs`) e i moduli `config`, `auth`,
`state`, `player`, `audio` e `ui` (`src/lib.rs`, usati anche dai test), più `library`, interno
alla libreria: è il primo pezzo del modulo `spotify` previsto qui sotto e per ora legge, con i
metadati di librespot, i brani di un album per `player`. L'interfaccia di riproduzione è
descritta in [PLAYER.md](PLAYER.md); la GUI nella sezione «Interfaccia grafica» qui sotto.

## Moduli previsti

| Modulo    | Responsabilità |
|-----------|----------------|
| `cli`     | Riga di comando (clap): analizza gli argomenti e smista verso `login`, `play`, `gui`. |
| `config`  | Configurazione utente: percorsi, qualità audio, preferenze; lettura e scrittura su disco. |
| `auth`    | Accesso con account Spotify Premium; salvataggio e rinnovo delle credenziali. |
| `spotify` | Dati Spotify: ricerca, libreria, playlist, metadati di brani, album e artisti. |
| `player`  | Logica di riproduzione: coda, play/pausa, salto, posizione, volume. |
| `audio`   | Motore audio: decodifica e uscita sul dispositivo di sistema. |
| `state`   | Stato condiviso dell'app (sessione, brano corrente, coda) e notifica dei cambiamenti. |
| `ui`      | Interfaccia grafica (egui/eframe): mostra lo stato e invia comandi, senza logica di dominio. |

Direzione delle dipendenze (mai al contrario):

- `cli` e `ui` sono i punti d'ingresso: danno i comandi a `state`, che decide, e passano
  a `player` l'esito (`PlaybackEffect`); gli eventi di `player` tornano a `state` tramite loro.
- `state` è logica pura: non dipende da nessun altro modulo, così si prova senza rete né audio.
- `player` usa `spotify` per i metadati e `audio` per il suono.
- `spotify` usa `auth` per la sessione; `auth` usa `config`.
- `config` non dipende da nessun altro modulo del progetto.

## Regola: il thread audio non blocca la UI

- La UI gira sul thread principale e deve restare sempre reattiva.
- Audio e rete girano su thread propri (o task asincroni), mai sul thread della UI.
- UI e player comunicano solo tramite canali: comandi in un verso, eventi di stato nell'altro.
- Nessuna chiamata bloccante dal thread della UI e nessun lock condiviso tenuto a lungo.
- Se l'audio è occupato (buffering, decodifica, rete lenta), la UI continua a rispondere
  e mostra lo stato di caricamento.
- Vale anche il contrario: un ridisegno lento della UI non deve interrompere l'audio.

## Interfaccia grafica (F4)

Codice in `src/ui/`; `src/main.rs` si limita a leggere gli argomenti di `gui` e a chiamare
`ui::run`. Due thread:

- **finestra** (thread principale): eframe/egui. Disegna e raccoglie clic e tasti; non fa
  rete, non tocca l'audio, non aspetta mai il motore.
- **motore** (`spotilite-engine`, `ui/engine.rs`): un runtime tokio tutto suo (2 worker)
  con la sessione Spotify (`player::Player`), la macchina a stati (`state::PlaybackState`) e
  le richieste in attesa della sessione. Parte quando la finestra esiste e si ferma quando
  la finestra si chiude (al massimo 3 s di attesa, poi il processo esce comunque).

Si parlano con due canali: `Command` dalla finestra al motore (canale tokio senza limite,
quindi l'invio non blocca: `TogglePlay`, `Next`, `Prev`, `SetVolume`, `Add`, `Reconnect`) ed
`EngineEvent` dal motore alla finestra (`std::sync::mpsc`, che la finestra svuota a ogni
frame senza attendere). Dopo ogni evento il motore chiama `ctx.request_repaint()`: la
finestra si ridisegna solo quando c'è qualcosa di nuovo, più la posizione ogni 500 ms
mentre un brano suona.

Il motore segue il ciclo di `spotilite play` (vedi [PLAYER.md](PLAYER.md)): ogni comando
passa dalla macchina a stati, l'esito va a `Player::apply`, e la finestra riceve una copia
dello stato; fine brano e brano non disponibile fanno `next()`, l'annuncio di fine brano
fa precaricare il seguente. Gli errori diventano `Problem` (`NotSignedIn`, `SignIn`,
`Audio`, `Network`, `Uri`, `Other`), cioè sono ordinati per ciò che l'utente può fare;
senza sessione la finestra offre «Riprova».

Il **modello di vista** (`ui/viewmodel.rs`, `ViewModel`) traduce eventi e stato in ciò che
la finestra mostra (titolo, artisti, tempi `m:ss`, stato, righe della coda, avvisi in
italiano) e controlla quello che l'utente scrive (`player::parse_request`, più i link
`open.spotify.com`). Non dipende da egui: si prova senza finestra (`tests/ui_view.rs`).
Il collegamento e il player stanno dietro i trait `Connector` e `Backend`: nei test un
finto (`ui/fake.rs`) prende il posto di Spotify, così il percorso comando -> canale ->
motore -> macchina a stati -> player -> canale -> modello di vista si prova senza rete né
audio, e la finestra stessa si disegna senza schermo con `egui::Context::run_ui`.

Grafica: prima OpenGL (glow), più leggero; se OpenGL 2.0 manca (schermi virtuali, alcune
sessioni remote) eframe riapre la finestra con wgpu: su Windows solo Direct3D 12 (con
Vulkan, su uno schermo virtuale, la finestra può restare bianca senza errori), su Linux
la scelta predefinita di wgpu; `WGPU_BACKEND` ha l'ultima parola. wgpu chiede
l'adattatore a basso consumo e `MemoryHints::MemoryUsage`: con il valore predefinito
riserverebbe blocchi da 128 e 64 MB, che con una GPU integrata sono RAM di sistema
(misure in [MISURE-f4.md](../MISURE-f4.md)).

## Motore audio e protocollo: librespot

Il motore audio e il protocollo Spotify si appoggeranno a `librespot` **0.8** come libreria
di terze parti (dipendenza da crates.io). Il codice di spotilite resta scritto da zero:
librespot si usa solo attraverso la sua API pubblica, senza copiarne il codice.

librespot resta confinato nei moduli `auth`, `spotify`, `player` e `audio`, dietro tipi
e interfacce di spotilite, così il resto dell'app non dipende direttamente dai suoi tipi:
`ui` usa quelli di `player` (`Player`, `PlaybackEvent`, `PlayerError`, `PlayRequest`), mai
quelli di librespot.

È tra le dipendenze dalla fase F1, insieme a `rodio` per l'uscita audio.

## Obiettivi trasversali

- Meno di 250 MB di RAM in uso normale.
- Windows e Linux.
- Errori propagati con `anyhow::Result`, niente `unwrap()` nei percorsi di esecuzione.
