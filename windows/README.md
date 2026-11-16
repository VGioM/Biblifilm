# Biblifilm pour Windows

La version Windows de Biblifilm : le même dossier analysé, la même identification en
ligne (TMDB avec clé, TVmaze et Wikidata sans), les mêmes tuiles par saison, la même
reprise de lecture et les mêmes épisodes vus. Une application native en Rust
([egui](https://github.com/emilk/egui)), tirée de la version Linux, avec **VLC**
comme lecteur, comme sur Mac.

## Installer

Télécharger `Biblifilm-<version>-windows-x64.zip` dans les
[releases](https://github.com/VGioM/Biblifilm/releases), le décompresser et lancer
`Biblifilm.exe`. **Rien d'autre à installer** : libVLC est dans le zip (dossier `vlc\`
à côté de l'exe). Les mises à jour se font depuis l'app (menu ··· → Rechercher les
mises à jour…).

## La lecture

Comme sur Mac avec VLCKit, l'écran devient le film : la vidéo est lue **dans la
fenêtre** par **libVLC 3.0** (le moteur de VLC). VLC décode, décodage matériel compris
(`--avcodec-hw=any`), et écrit chaque image dans un tampon ; Biblifilm l'envoie dans une
texture OpenGL et la copie dans la fenêtre à l'échelle, puis dessine la barre de lecture
par-dessus. Il lit tout : MKV, x265 10 bits, AV1, EAC3/DTS, sous-titres ASS. L'écran
reste allumé pendant la lecture.

- Reprise à la position enregistrée, épisode vu au-delà de 92 %, lecture comptée
  seulement après 3 secondes vraiment jouées.
- La saison s'enchaîne ; « Épisode suivant » passe au suivant.
- Pistes audio et sous-titres au choix ; le choix est retenu (par titre et langue) et
  réappliqué à l'épisode suivant. Le français est préféré, puis l'anglais.
- Clavier : Espace lecture/pause, ←/→ 10 s, ↑/↓ et molette pour le volume, B piste audio
  suivante, V sous-titres suivants, N épisode suivant, F ou double-clic plein écran,
  Échap pour revenir.

« Ouvrir dans VLC » lance le `vlc.exe` livré dans le zip (ou VLC installé), sans suivi.

libVLC est chargée au lancement, dans un thread : d'abord `vlc\libvlc.dll` à côté de
l'exe, sinon celle de VLC installé (`C:\Program Files\VideoLAN\VLC`, version 64 bits).

## Autres différences

- Le dossier est surveillé par Windows (ReadDirectoryChangesW) ; les événements sont
  regroupés sur quelques secondes.
- Données : `%APPDATA%\Biblifilm\` (`settings.json`, `metadata-cache.json`,
  `watch-state.json` et sa sauvegarde). Jaquettes en cache dans `%LOCALAPPDATA%\Biblifilm\`.
- `BIBLIFILM_SUPPORT_DIR` pointe les données ailleurs, comme sur Mac.
- Raccourcis : Ctrl+O dossier, Ctrl+R analyser, Ctrl+F chercher, Ctrl+, réglages,
  Ctrl+I informations, Alt+← retour, Entrée lire.

## Développer

Il faut Rust stable (cible `x86_64-pc-windows-msvc`). Dans `windows/` :

```sh
./fetch-libvlc.sh        # libVLC 3.0.21 officielle (VideoLAN) dans extra/vlc, empreinte vérifiée
cargo run --release      # lance l'app (elle trouve extra/vlc toute seule)
cargo test               # tests unitaires
./build-windows.sh       # dist/Biblifilm-<version>-windows-x64.zip, avec libVLC
```

`BF_DEBUG=1` écrit l'état du lecteur sur la sortie d'erreur ; `VLC_TRACE=1` ajoute le
journal de VLC.

`src/common/` est le même dossier dans Biblifilm, Biblijeu et Recomps.
