# Architettura di spotilite

Riferimento per le fasi dopo F0. Descrive i moduli previsti e le regole che li legano.
Oggi il codice contiene la riga di comando (`src/main.rs`) e i moduli `config` e `auth`
(`src/lib.rs`, usati anche dai test).

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
| `ui`      | Interfaccia grafica: mostra lo stato e invia comandi, senza logica di dominio. |

Direzione delle dipendenze (mai al contrario):

- `cli` e `ui` sono i punti d'ingresso: leggono `state` e inviano comandi a `player`.
- `player` usa `spotify` per i metadati e `audio` per il suono, e aggiorna `state`.
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

## Motore audio e protocollo: librespot

Il motore audio e il protocollo Spotify si appoggeranno a `librespot` **0.8** come libreria
di terze parti (dipendenza da crates.io). Il codice di spotilite resta scritto da zero:
librespot si usa solo attraverso la sua API pubblica, senza copiarne il codice.

librespot resta confinato nei moduli `auth`, `spotify`, `player` e `audio`, dietro tipi
e interfacce di spotilite, così il resto dell'app non dipende direttamente dai suoi tipi.

È tra le dipendenze dalla fase F1, insieme a `rodio` per l'uscita audio.

## Obiettivi trasversali

- Meno di 250 MB di RAM in uso normale.
- Windows e Linux.
- Errori propagati con `anyhow::Result`, niente `unwrap()` nei percorsi di esecuzione.
