# Biblifilm pour Linux

La version Linux de Biblifilm : le même dossier analysé, la même identification en
ligne (TMDB avec clé, TVmaze et Wikidata sans), les mêmes tuiles par saison, la même
reprise de lecture et les mêmes épisodes vus. Une application native en Rust (l'app Mac
est en SwiftUI avec VLCKit, qui n'existent pas sur Linux), distribuée en AppImage.

## Installer

Télécharger `Biblifilm-<version>-linux-x86_64.AppImage` dans les
[releases](https://github.com/VGioM/Biblifilm/releases), le rendre exécutable
(`chmod +x`) et le lancer. Les mises à jour se font ensuite depuis l'app (menu ··· →
Rechercher les mises à jour…). **Rien d'autre à installer** : le lecteur est dans
l'AppImage.

## La lecture

Comme sur Mac, l'écran devient le film : la vidéo est lue **dans la fenêtre** par
**libmpv** (le moteur de mpv), inclus dans l'AppImage avec son FFmpeg complet. Il lit
tout — MKV, x265 10 bits, AV1, EAC3/DTS, sous-titres ASS — avec le décodage matériel
quand la carte graphique le permet (`hwdec=auto-copy-safe`), et tient l'écran allumé
pendant la lecture.

- Reprise à la position enregistrée, épisode vu au-delà de 92 %, lecture comptée
  seulement après 3 secondes vraiment jouées, position écrite régulièrement et en
  quittant.
- La saison s'enchaîne ; « Épisode suivant » passe au suivant.
- Pistes audio et sous-titres au choix dans la barre de lecture ; le choix est retenu
  (par libellé) et réappliqué à l'épisode suivant.
- Clavier : Espace lecture/pause, ←/→ 10 s, ↑/↓ et molette pour le volume, B piste audio
  suivante, V sous-titres suivants, N épisode suivant, F ou double-clic plein écran,
  Échap pour revenir.

« Ouvrir dans VLC » (VLC en paquet ou en Flatpak, sinon l'app par défaut) reste la porte
de sortie, sans suivi.

## Autres différences

- Le dossier est surveillé par inotify au lieu de FSEvents ; les événements sont
  toujours regroupés sur quelques secondes.
- Données : `~/.config/Biblifilm/` (`settings.json`, `metadata-cache.json`,
  `watch-state.json` et sa sauvegarde) — les mêmes fichiers que la première version
  Linux, repris tels quels. Jaquettes en cache dans `~/.cache/Biblifilm/`.
- `BIBLIFILM_SUPPORT_DIR` pointe les données ailleurs, comme sur Mac.
- Raccourcis : Ctrl+O dossier, Ctrl+R analyser, Ctrl+F chercher, Ctrl+, réglages,
  Ctrl+I informations, Alt+← retour, Entrée lire.

## Développer

Une application Rust native ([egui](https://github.com/emilk/egui), rendu OpenGL,
Wayland et X11). Il faut Rust stable, et pour l'AppImage une Fedora — sur un système
immuable (Bazzite, Silverblue, Kinoite), un conteneur toolbox Fedora fait l'affaire :

```sh
toolbox create -c rustbuild --distro fedora --release 44
toolbox enter rustbuild
sudo dnf install -y gcc pkgconf-pkg-config squashfs-tools patchelf \
    wayland-devel libxkbcommon-devel libxkbcommon-x11 libX11-devel libXcursor libXrandr libXi mesa-libEGL-devel \
    mpv-devel    # libmpv, de RPM Fusion (dnf install rpmfusion-free-release d'abord)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh    # Rust, si absent
```

Puis, dans `linux/` :

```sh
cargo run --release      # lance l'app
cargo test               # tests unitaires
./build-appimage.sh      # dist/Biblifilm-<version>-linux-x86_64.AppImage
```

`build-appimage.sh` compile, copie dans l'AppImage les bibliothèques dont le programme
a besoin — jamais la glibc, les pilotes graphiques, le son ni les bibliothèques de
base du bureau, qui restent celles du système — et la compresse (squashfs xz). Le
« runtime » AppImage placé en tête est pris dans `APPIMAGE_RUNTIME` (par défaut : celui
qu'electron-builder a mis en cache).

`src/common/` est le même dossier dans Biblifilm, Biblijeu et Recomps : thème, icônes,
cache des jaquettes (disque `~/.cache/Biblifilm/`, mémoire bornée), fenêtres de choix
(portail XDG), réglages, mise à jour intégrée (`selfupdate.rs`, l'équivalent de
`SelfUpdate.swift` : elle ne prend que le fichier `-linux-x86_64.AppImage` de la
release, vérifie son empreinte SHA-256 publiée par GitHub, puis remplace l'AppImage).

## Publier

```sh
cd linux && ./release-linux.sh 1.4.0    # l'AppImage, dans la release v1.4.0 (créée si besoin)
```

Mac et Linux partagent les releases du dépôt, pas leurs numéros : une release peut
contenir le zip Mac, l'AppImage, ou les deux. Chaque version ne prend que son fichier
(`-macOS-arm64.zip` ou `-linux-x86_64.AppImage`) et se met à jour vers la release la
plus récente qui le contient ; une release Mac seule ne cache pas une version Linux,
et inversement. Un numéro (un tag) ne sert qu'une fois.
