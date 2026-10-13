# Misure F4: consumo della GUI

Data: **2026-10-13**, ore 18:40 circa. Macchina: `DESKTOP-CMPK1TG`, Windows 10 Pro 19045,
AMD Ryzen 3 PRO 4350GE con Radeon Graphics integrata, 15,4 GB di RAM.
Binario: `D:\spotilite\build\target\f4\release\spotilite.exe` (profilo release, 23,1 MB),
commit `db103ca` più la sola documentazione di F4-5.

## Cosa si misura

`WorkingSet64` del processo `spotilite gui` con la finestra aperta e una coda caricata,
**senza riproduzione reale**: su questa macchina non c'è un token di un account Premium, quindi
la sessione Spotify non si apre, il dispositivo audio non viene aperto e la coda resta «in
attesa della sessione». È la memoria della finestra, del motore (thread e runtime tokio,
macchina a stati, coda) e del driver grafico; non c'è quella di librespot in riproduzione.

Su questa macchina lo schermo attivo è virtuale («USB Mobile Monitor Virtual Display») e
OpenGL c'è solo nella versione 1.1: la finestra usa quindi il ripiego **wgpu con Direct3D 12**
(sulla Radeon integrata). Il percorso principale, OpenGL con glow, qui non si può misurare.

## Metodo

Script PowerShell `D:\spotilite\build\f4-scripts\measure.ps1`:

```
$env:APPDATA = 'D:\spotilite\build\f4-manual'      # nessun token: niente sessione, niente audio
Start-Process spotilite.exe -ArgumentList gui, <N URI> -PassThru
# a 5, 15, 30 e 60 s dall'avvio: $p.Refresh(); $p.WorkingSet64, PrivateMemorySize64, PeakWorkingSet64
$p.CloseMainWindow()                                # come il pulsante X; poi il codice d'uscita
```

La coda passa dalla riga di comando (`spotilite gui <uri>...`), ripetendo cinque voci: tre
brani, un episodio, un album (`spotify:track:4uLU6hMCjMI75M1A2tKUQC`,
`spotify:track:6rqhFgbbKwnb9MLmUQDhG6`, `spotify:track:0VjIjW4GlUZAMYd2vXMi3b`,
`spotify:episode:512ojhOuo1ktJprKbVcKyQ`, `spotify:album:6akEvsycLGftJxYudPjmqK`).
Comando: `powershell -File measure.ps1 -Exe ...\release\spotilite.exe -QueueLength N`.
Misure prese a macchina ferma (nessuna compilazione in corso).

## Risultati (release)

| Coda | a 5 s | a 15 s | a 30 s | a 60 s | Privati a 60 s | Picco | Uscita |
|-----:|------:|-------:|-------:|-------:|---------------:|------:|-------:|
| 0 voci | 178,8 MB | 179,1 MB | 179,1 MB | **179,1 MB** | 227,2 MB | 179,1 MB | 0 |
| 50 voci | 179,7 MB | 179,9 MB | 179,9 MB | **179,9 MB** | 226,4 MB | 179,9 MB | 0 |
| 500 voci | 180,6 MB | 180,9 MB | 180,9 MB | **180,9 MB** | 228,4 MB | 180,9 MB | 0 |

(MB = 2^20 byte.) Il consumo resta fermo nel tempo e la coda costa circa 2 MB ogni 500
voci: la lista disegna solo le righe visibili. 33-35 thread.

CPU a finestra ferma (coda di una voce, nessun evento): **0 ms** di tempo di processore in
30 s (`TotalProcessorTime`, script `idle-cpu.ps1`). La finestra si ridisegna solo quando il
motore ha qualcosa di nuovo; mentre un brano suona si aggiunge un aggiornamento della
posizione ogni 500 ms (non misurato: serve la riproduzione).

## Da dove vengono i 179 MB

- **Prima misura, con le impostazioni predefinite di wgpu: 255,3 MB** di working set e 409 MB
  privati (stessa macchina, `spotilite gui` senza coda, a 4 s). wgpu, tramite gpu-allocator,
  riserva la memoria della GPU in blocchi da 128 MB (dispositivo) e 64 MB (host): con una GPU
  integrata sono RAM di sistema. Con `MemoryHints::MemoryUsage` (blocchi da 8 e 4 MB) si
  scende a **179,0 MB** e 226,8 MB privati.
- Dei 68 moduli caricati (178,5 MB d'immagini mappate, contate solo in parte nel working
  set) il più grosso è `amdxc64.dll`, il compilatore di shader del driver AMD (69,8 MB), poi
  `spotilite.exe` (22,4 MB), `windows.storage.dll` e `shell32.dll` (7,5 MB circa ciascuno),
  `d3d10warp.dll` (7 MB: wgpu enumera anche l'adattatore software WARP), `d3dcompiler_47.dll`
  (4,4 MB). La parte grossa che resta è quindi del driver grafico Direct3D 12.
- Build debug, per confronto: 268,6 MB (prima di `MemoryUsage`), 208-210 MB dopo.

## Rispetto all'obiettivo

Obiettivo del progetto: meno di 250 MB di RAM in uso normale. La finestra con coda caricata
sta a 179-181 MB su questo percorso (Direct3D 12, GPU integrata AMD). Manca la quota della
riproduzione (sessione librespot, decodifica, buffer audio), che nessuna fase ha ancora
misurato: va misurata dall'utente con un account Premium, per esempio con lo stesso script
dopo `spotilite login` e con un brano in ascolto. Da misurare anche il percorso OpenGL su
uno schermo collegato a una GPU con driver OpenGL completo.

## Altri numeri di F4-5

- `cargo build --release`: 8m 17s da cartella target vuota (con eframe e wgpu); 1m 46s
  incrementale per la build finale. Nessun warning.
- `spotilite gui --selftest` (5 frame) -> exit 0 in 0,93 s; `--selftest 5` -> exit 0 in
  0,50 s; `--selftest 60` -> exit 0 in 1,56 s; `gui spotify:album:... --selftest` -> exit 0;
  `--selftest 0` -> exit 2 (errore d'uso).
- Chiusura con `CloseMainWindow` (come il pulsante X): exit 0 in 0,12-0,20 s.
