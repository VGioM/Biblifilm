# Biblifilm pour Linux

La version Linux de Biblifilm : le même dossier analysé, la même identification en
ligne (TMDB avec clé, TVmaze et Wikidata sans), les mêmes tuiles par saison, la même
reprise de lecture et les mêmes épisodes vus. Écrite en Electron (l'app Mac est en
SwiftUI avec VLCKit, qui n'existent pas sur Linux), distribuée en AppImage.

## Installer

Télécharger `Biblifilm-<version>-linux-x86_64.AppImage` dans les
[releases](https://github.com/VGioM/Biblifilm/releases), le rendre exécutable
(`chmod +x`) et le lancer. Les mises à jour se font ensuite depuis l'app (menu Aide).

**mpv est installé tout seul.** Au premier lancement, si mpv est absent, Biblifilm
l'installe depuis Flathub (`io.mpv.Mpv`, pour votre compte : aucun mot de passe). En
cas d'échec, le bouton « Installer mpv » de Réglages → Lecture et de l'écran d'erreur du
lecteur intégré recommence. Un mpv déjà installé (paquet de la distribution ou Flatpak)
est utilisé tel quel.

## La lecture

Sur Mac, le lecteur est intégré (VLCKit). Sur Linux, deux lecteurs :

- **mpv** (installé en paquet ou en Flatpak), piloté par son socket IPC. Il lit tout
  (MKV, x265 10 bits, EAC3, sous-titres ASS), tient l'écran allumé et gère clavier et
  plein écran. Biblifilm lui passe la position de reprise et la suite de la saison,
  donne à chaque épisode son vrai titre, et lit la position toutes les 2 secondes :
  reprise, épisode vu au-delà de 92 %, lecture comptée seulement après 3 secondes
  vraiment jouées, dernière écriture en quittant. Les pistes audio et sous-titres
  choisies sont reconnues à leur libellé et réappliquées à l'épisode suivant.
- **Le lecteur intégré**, en attendant mpv (ou s'il est choisi dans Réglages → Lecture) :
  l'écran devient le film, comme sur Mac. Il lit ce que Chromium décode (MP4, WebM, la
  plupart des MKV en H.264/VP9/AV1 avec AAC ou Opus) ; un fichier qu'il ne sait pas lire
  affiche une erreur et « Ouvrir dans un lecteur externe » au lieu de compter l'épisode
  comme vu. Commandes : Espace, ←/→ 10 s, ↑/↓ et molette pour le volume, B piste audio
  suivante, F ou double-clic plein écran, Échap.

« Ouvrir dans VLC » (VLC en paquet ou en Flatpak, sinon l'app par défaut) reste la porte
de sortie, sans suivi.

## Autres différences

- Le dossier est surveillé avec `fs.watch` (inotify) au lieu de FSEvents ; les
  événements sont toujours regroupés sur 4 secondes.
- Données : `~/.config/Biblifilm/` (`settings.json`, `metadata-cache.json`,
  `watch-state.json` et sa sauvegarde `watch-state.backup.json`). Les jaquettes sont
  gardées dans le cache disque de l'app (« Vider le cache » dans les réglages).
- `BIBLIFILM_SUPPORT_DIR` pointe les données ailleurs, comme sur Mac.
- Raccourcis : Ctrl+O dossier, Ctrl+R analyser, Ctrl+, réglages, Ctrl+I informations,
  Alt+← retour, Entrée lire.

## Développer

```sh
cd linux
npm install
npm start
npm test             # parseur (mêmes cas que FilenameParserTests.swift), saisons, reprise, mpv
npm run dist         # dist/Biblifilm-<version>-linux-x86_64.AppImage
```

- `src/parser.js` : `FilenameParser.swift`
- `src/metadata.js` : `MetadataService`, `TMDBProvider`, `TVmazeProvider`
- `src/shared.js` : `MediaItem`, `LibraryEntry`, `WatchStore` (partie pure), sections
- `src/player.js` : la séance mpv ; `test/fake-mpv.js` imite mpv pour les tests
- `src/renderer/player.js` : le lecteur intégré

## Publier

```sh
cd linux && ./release-linux.sh 1.4.0   # ajoute l'AppImage à la release v1.4.0 existante
```
