# Biblifilm pour Linux

La version Linux de Biblifilm : le même dossier analysé, la même identification en
ligne (TMDB avec clé, TVmaze et Wikidata sans), les mêmes tuiles par saison, la même
reprise de lecture et les mêmes épisodes vus. Une application native en Rust (l'app Mac
est en SwiftUI avec VLCKit, qui n'existent pas sur Linux), distribuée en AppImage, qui
lit tout dans **VLC**.

## Installer

Télécharger `Biblifilm-<version>-linux-x86_64.AppImage` dans les
[releases](https://github.com/VGioM/Biblifilm/releases), le rendre exécutable
(`chmod +x`) et le lancer. Les mises à jour se font ensuite depuis l'app (menu ··· →
Rechercher les mises à jour…). **Rien d'autre à installer** : s'il manque, Biblifilm
installe VLC lui-même à la première lecture (voir ci-dessous).

## La lecture

Un clic sur **Lire** ouvre le film dans **VLC**, en plein écran (réglable dans
Réglages → Lecture). Biblifilm suit la lecture par l'interface web locale de VLC
(uniquement sur 127.0.0.1, avec un port et un mot de passe tirés au hasard à chaque
lecture) ; une barre « En lecture dans VLC » montre le titre et la position, avec
« Fermer VLC ».

- Reprise à la position enregistrée, épisode vu à la fin (ou au-delà de 92 %), lecture
  comptée seulement après 3 secondes vraiment jouées, position écrite régulièrement,
  à la pause et à la fermeture de VLC.
- La saison s'enchaîne dans la liste de lecture de VLC ; « Suivant » dans VLC passe à
  l'épisode suivant, et Biblifilm suit.
- L'audio français est choisi en priorité ; pistes et sous-titres se changent dans VLC.

### VLC installé et tenu à jour

Biblifilm utilise le **Flatpak officiel de VLC** ([Flathub](https://flathub.org/apps/org.videolan.VLC)),
la façon propre d'installer une application sur un système immuable comme Bazzite,
Silverblue ou Kinoite (pas de paquet à superposer avec rpm-ostree) :

- S'il n'est pas là, il est installé à la première lecture (≈ 60 Mo, plus le runtime
  KDE s'il manque), pour tout le système — sans mot de passe pour un administrateur
  sur Fedora/Bazzite — ou, à défaut, pour l'utilisateur seul. La lecture démarre
  ensuite toute seule.
- Il est mis à jour automatiquement une fois par jour au lancement de Biblifilm
  (Réglages → Lecture → « Mettre à jour VLC maintenant » pour forcer).
- Un VLC installé autrement (paquet de la distribution) est utilisé s'il n'y a pas
  de Flatpak ; ses mises à jour restent celles de la distribution.

« Ouvrir avec une autre application » passe par l'application par défaut du bureau,
sans suivi.

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
sudo dnf install -y gcc pkgconf-pkg-config squashfs-tools \
    wayland-devel libxkbcommon-devel libxkbcommon-x11 libX11-devel libXcursor libXrandr libXi mesa-libEGL-devel
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh    # Rust, si absent
```

Puis, dans `linux/` :

```sh
cargo run --release      # lance l'app
cargo test               # tests unitaires
./build-appimage.sh      # dist/Biblifilm-<version>-linux-x86_64.AppImage
```

`build-appimage.sh` compile et emballe le programme dans l'AppImage (squashfs gzip) ;
il n'embarque aucune bibliothèque : la lecture est confiée à VLC. Le
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
