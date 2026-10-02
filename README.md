# Biblifilm

A macOS app that scans a folder for anything VLC can play, identifies each file
online (full title, episode name, synopsis, cover art) and plays it in VLC on click.

Styled after Music.app in dark mode.

## Build

```bash
./build.sh          # fetches VLCKit if needed, produces dist/Biblifilm.app
./install.sh        # optional: copy it into /Applications
open dist/Biblifilm.app
```

The first build downloads VLCKit (~84 Mo) into `Frameworks/`.

## Sauvegarder et déplacer le projet

```bash
./package.sh                        # sources seules
./package.sh --with-data            # + réglages, historique, jaquettes
./package.sh --with-data --with-key # + la clé TMDB
```

Produit `~/Desktop/Biblifilm-<date>.tar.gz` (~5 Mo), contenant un `RESTORE.md`
qui explique la remise en route. VLCKit n'est jamais inclus : `build.sh` le
retélécharge. La clé TMDB est exclue par défaut.

To keep it around: drag `dist/Biblifilm.app` into `/Applications`.

Requires Xcode command line tools. VLC is optional — without it, files open in
whatever app macOS associates with them.

## First run

1. Click **Choisir un dossier…** (bottom of the sidebar) and pick your media folder.
   Subfolders are scanned too.
2. Open **Réglages → Métadonnées** (⌘,) and paste a TMDB API key.
   Free from <https://www.themoviedb.org/settings/api>. Click **Tester la clé** to
   confirm it works.

Pasting a key is enough on its own: the app notices, forgets every recorded miss
and re-runs those lookups automatically. Successful matches are kept, so it only
retries what actually failed.

## Nouveaux fichiers : rien à faire

Le dossier est surveillé en continu (FSEvents, récursif). Un film ajouté pendant
que l'app tourne apparaît de lui-même en quelques secondes, déjà identifié avec
sa jaquette — sans relance ni analyse manuelle. Les événements sont regroupés
sur 4 secondes pour qu'un téléchargement en cours ne déclenche pas dix analyses.

L'analyse est incrémentale : les fiches déjà résolues sont conservées, seuls les
nouveaux fichiers sont cherchés en ligne. Désactivable dans **Réglages → Général**.

Either credential TMDB shows you works: the short v3 *API Key* or the long v4
*API Read Access Token*.

### Without a TMDB key

The app still works, falling back to TVmaze + Wikidata. That covers **TV shows
only** — films stay unidentified, and the app shows a banner saying so. Wikidata
is used to translate a localized title into the name TVmaze indexes, which is how
`Nicky Larson` resolves to `City Hunter`.

TVmaze is deliberately never consulted for films: it is a television database, so
searching it for a film returns confident-looking nonsense (`Black Hawk Down`
matched the documentary series *Surviving Black Hawk Down*). A wrong match is
worse than none, so films wait for TMDB.

When several shows share a name, the year in the filename decides:
`Monster.2004.S01E01` resolves to the 2004 anime, not the 2016 Korean drama.

### Ranking

TMDB's own result order is not reliable for exact titles — searching
`Independence Day` returns *Resurgence* (2016) above the 1996 original, and
`Monster` returns *Monster House* first. Results are therefore re-ranked here on
exact title agreement (accent- and punctuation-insensitive, against both the
localized and original title) and on the filename's year, with popularity used
only to break ties.

## What it does with filenames

Release names are parsed before any lookup — release groups, resolution, codec,
language and source tags are stripped:

| Filename | Detected |
|---|---|
| `[DragonMax] Nicky Larson - 001.mkv` | *Nicky Larson*, absolute episode 1 |
| `Breaking.Bad.S01E02.1080p.WEB-DL.x264-GRP.mkv` | *Breaking Bad*, S01E02 |
| `The Wire 3x04 - Amsterdam.avi` | *The Wire*, S03E04 |
| `Inception.2010.1080p.BluRay.x264.mkv` | *Inception* (2010), film |
| `Naruto Shippuden Ep 220 VOSTFR.mp4` | *Naruto Shippuden*, absolute episode 220 |
| `Twin Peaks/S01E03.mkv` | *Twin Peaks*, S01E03 (title from the folder) |
| `Monster.2004.S01E61.MULTi.WEB-FW.mkv` | *Monster* (2004), S01E61 |
| `Black Hawk Down  ( La Chute du faucon noir).avi` | *Black Hawk Down* |
| `Blade Runner 2049 (2017) 1080p.mkv` | *Blade Runner 2049* (2017) |

Anime-style absolute numbering (`- 001`) is mapped onto real seasons using the
show's season lengths, so episode 001 becomes S01E01 and the episode's actual
title is shown.

When the filename carries no show name, the parent folder is used instead —
unless the folder is only `Season 01`-style wording.

Got a wrong match? Select the item and click **Corriger** to type the real title
and re-run the lookup for that file alone.

## Séries : une tuile par saison

Les épisodes ne s'affichent pas un par un. Ils sont regroupés en **une tuile par
saison**, avec un effet de pile et le nombre d'épisodes en pastille.

- **Clic** sur une saison → la page de la saison : jaquette, synopsis, et la
  liste des épisodes. Cliquez un épisode pour le lancer (ou double-clic sur la
  ligne ; la sélection affiche ses infos à droite).
- **Lire la saison** enchaîne tous les épisodes dans une seule liste VLC.
- ⌘[ ou la flèche ← de la barre d'outils pour revenir à la bibliothèque.

### Films, OAV et bonus

Les hors-séries ne sont pas mélangés aux épisodes. `Nicky Larson - OAV - 01 -
Bay City Wars` allait sinon se faire passer pour l'épisode 1 et entrait en
collision avec le vrai. Ils forment leur propre tuile :

- **Films et hors-séries** : films, téléfilms, OAV, spéciaux
- **Bonus** : openings, endings, NCOP/NCED, génériques, making-of

Chaque ligne porte son vrai titre (« Bay City Wars », « La Mort de Ryo Saeba »),
pas le nom de fichier.

Un marqueur n'est reconnu qu'après un tiret séparateur, et les mots ambigus
(`Film`, `Movie`, `Special`) exigent en plus un numéro — sans quoi *The Movie
Critic* ou *Film Noir Collection* seraient pris pour des hors-séries.

Le tri **par Titre** classe sur le titre de la série ou du film — jamais sur le
titre de l'épisode — puis par numéro de saison.

## Reprise de lecture et épisodes vus

Biblifilm suit où vous en êtes, façon Netflix, et l'état survit à la fermeture
de l'app.

- Une **barre de progression** rose court en bas de la jaquette d'un titre commencé.
- Une **pastille ✓** marque un film ou une saison entièrement vus.
- Une saison affiche **« 5/28 vus »** et une progression cumulée.
- Le bouton devient **« Reprendre à 21:07 »**, ou **« Reprendre l'épisode 6 »**
  pour une saison — « Lire la saison » démarre au premier épisode non vu.
- La section **Reprendre** dans la barre latérale liste ce qui est commencé mais
  pas fini, le plus récent d'abord.
- Menu contextuel : marquer vu / non vu, à l'unité ou sur toute une saison,
  et « Lire depuis le début ».

La file d'attente d'un épisode vient toujours de sa **saison entière**, jamais
de ce qui est affiché : sous « Reprendre », une tuile ne contient que les
épisodes en cours, et lancer la lecture depuis là ne donnait donc aucune suite.

Un épisode est considéré comme vu au-delà de 92 % (le reste, c'est le générique),
et la reprise est proposée dès 5 secondes de lecture. La position part du moteur
lui-même (pas d'un compteur mis à jour par notifications), elle est écrite au
maximum toutes les 2 secondes et seulement si elle a bougé — et une dernière
fois en quittant l'app, sinon un ⌘Q en pleine lecture perdait jusqu'à deux
secondes, ce qui suffit à faire disparaître une reprise en début d'épisode.

### Comment ça marche

Le lecteur est **intégré à l'app** : l'écran devient le film, sans fenêtre VLC
séparée. Le moteur est VLCKit (le cœur de VLC), donc tous vos codecs
fonctionnent — MKV, x265 10 bits, EAC3. L'état est écrit dans
`~/Library/Application Support/Biblifilm/watch-state.json`.

Commandes :

| | |
|---|---|
| **Deux doigts haut / bas** | volume (pas de 5, avec affichage à l'écran) |
| Espace | lecture / pause |
| ← / → | reculer / avancer de 10 s |
| ↑ / ↓ | volume |
| B | piste audio suivante |
| V | sous-titres suivants (dont « aucun ») |
| Échap | quitter le plein écran, sinon retour à la bibliothèque |
| Double-clic | plein écran |
| Clic | pause |

Échap suit la logique du système : en plein écran il rend d'abord sa taille
normale à la fenêtre, et c'est seulement depuis une fenêtre normale qu'il quitte
l'épisode. Un menu de pistes ouvert se referme en premier.

Les boutons **≋** et **💬** de la barre de lecture listent les pistes audio et
les sous-titres du fichier, avec « Aucun » en tête pour les sous-titres. VLC ne
les connaît qu'une fois le flux analysé : les listes se remplissent une seconde
après le démarrage, et sont relues sur le même tic de 400 ms que l'état de
lecture (une piste de sous-titres externe peut arriver plus tard encore).

Le choix suit la file d'attente. VLC renumérote les pistes à chaque fichier, donc
c'est le libellé (« Français VFi AC3 5.1 @ 448Kbps ») qui est mémorisé et
recherché dans l'épisode suivant. Il est réappliqué tant que la lecture n'a pas
vraiment démarré, parce que VLC peut activer sa propre piste par défaut juste
après avoir publié la liste ; une sélection manuelle l'annule, elle ne se fait
donc jamais écraser.

Tant qu'un film est **réellement en lecture**, l'app tient une assertion
`NoDisplaySleep` (la même que VLC) : l'écran ne s'éteint pas, l'économiseur ne
démarre pas et le Mac ne se verrouille pas au milieu d'un épisode. Elle est
relâchée dès la pause et en quittant le lecteur — un film en pause n'a aucune
raison de garder le Mac éveillé toute la nuit. `pmset -g assertions` la montre
pendant la lecture.

Les contrôles s'effacent après 3 secondes de lecture, **et le pointeur avec
eux** ; le moindre mouvement les ramène. Le geste de volume affiche seulement
son propre indicateur, sans réveiller la barre de lecture. À la fin d'un
épisode, le suivant démarre automatiquement.

`isPlaying` est relu toutes les 400 ms sur le moteur. Les notifications d'état
de VLCKit se perdent, et les tics de temps s'arrêtent en pause : un `isPlaying`
resté à « vrai » faisait rappeler `pause()`, qui « ne fait rien si déjà en
pause » — la lecture ne repartait jamais.

`VLCVideoView` ne gère aucune entrée — les gestes de VLC.app viennent de son
interface, pas du moteur. Ils sont donc réimplémentés ici sur `scrollWheel`,
en distinguant le trackpad (deltas précis) de la molette.

La fin d'un épisode est détectée **à l'horloge**, pas sur l'état du moteur :
VLCKit 3.7 n'émet jamais `.ended` pour ces fichiers, il se gare sur `paused`
à ~180 ms de la durée totale. Attendre cet état revenait à ne jamais terminer
un épisode, donc à ne jamais lancer le suivant. La détection demande donc :
lecture réellement commencée, moteur à l'arrêt, position à moins de 2 secondes
de la fin, et **pas de pause volontaire** — sans quoi une pause sur la dernière
image passerait pour une fin d'épisode.

Un fichier illisible ne peut pas contaminer l'historique : la lecture n'est
comptée que si plus de 3 secondes ont réellement été jouées, et un fichier qui
ne démarre pas au bout de 15 secondes affiche une erreur avec une porte de
sortie « Ouvrir dans VLC ». Sans cette garde, un fichier en échec signale « fin »
instantanément et la file d'attente marque toute une saison comme vue.

**Ouvrir dans VLC** reste disponible partout (menu contextuel, panneau
d'informations) si un fichier résiste au lecteur intégré.

VLCKit 3.7.3 est récupéré par `./fetch-vlckit.sh` (~84 Mo) dans `Frameworks/`,
hors du dépôt, et embarqué dans le bundle par `build.sh` — qui signe le
framework avant l'app, dans cet ordre obligatoire. Le bundle pèse **82 Mo**.
AVPlayer, l'alternative native, est hors-jeu : il ne lit pas le MKV.

## Usage

- **Double-click** a poster, or select it and press **Return**, to play in VLC.
- **Tout lire** (toolbar, top left) queues everything currently shown as one VLC playlist.
- **Right-click** for play / reveal in Finder / re-lookup / copy path.
- The sidebar filters by Films, Séries, Audio, recent additions, unidentified
  files, and one entry per detected series.
- ⌘O choose folder · ⌘R rescan · ⌘⇧R reveal in Finder · ⌘, settings.

## Notes

- Files under 20 MB are skipped by default (samples and trailers). Toggle in Réglages.
- Metadata and cover art are cached in
  `~/Library/Application Support/Biblifilm/`, so relaunches are instant and the
  network is only hit for new files. Both caches can be cleared in Réglages.
- The metadata cache is written every 25 lookups, so quitting during the first
  scan of a large library does not lose the work already done. The final flush
  on quit runs in a **detached** task: a plain `Task` inherits the main actor,
  which `applicationWillTerminate` was blocking on a semaphore, so the flush
  never started and every quit waited out the full two-second timeout (measured
  at 2,11 s) with the cache unwritten.
- Failed lookups are cached too, so the app doesn't retry them on every launch.
  Use **Tout rechercher à nouveau** to force a full refresh.
- **Rechercher les infos à nouveau** (menu contextuel) drops that title's cache
  entry before searching again. It used to go through the cache, which handed
  back the very answer the user was trying to replace.
- A collection numbered by hand ("Rocky I") is searched again without its
  trailing numeral when nothing matches by title: TMDB files the first film as
  "Rocky", and searching "Rocky I" returns two obscure 1976 shorts instead.
  The fallback only fires after the real title fails, so "Star Wars Episode I"
  is left alone.
- The app is ad-hoc signed. It is not sandboxed, so it can read any folder you
  point it at.

## Layout

```
Sources/Biblifilm/
  BiblifilmApp.swift        app entry, menu commands
  Models/MediaItem.swift    file + parse result + resolved metadata
  Models/LibraryEntry.swift grid tiles: a film, or a season of episodes
  Core/
    FilenameParser.swift    release name -> title, season/episode, year
    FolderWatcher.swift     FSEvents watch so new files appear on their own
    WatchStore.swift        resume positions + watched state, persisted
    PlayerModel.swift       the embedded player: VLCKit engine + progress
  Views/
    PlayerScreen.swift      full-window playback UI + keyboard monitor
    VideoSurface.swift      VLC render target + trackpad gestures
    LibraryScanner.swift    recursive folder walk
    LibraryModel.swift      state, filtering, sorting, lookup orchestration
    ArtworkCache.swift      cover art, memory + disk
    AppSettings.swift       preferences
    VLCLauncher.swift       opens files in VLC
  Metadata/
    MetadataService.swift   provider chain, request coalescing, disk cache
    TMDBProvider.swift      primary (needs key)
    TVmazeProvider.swift    keyless fallback, Wikidata title resolution
  Views/
    LibraryGridView.swift   tile grid, missing-key banner
    EntryCard.swift         one tile (stacked look for seasons)
    SeasonDetailView.swift  season page + episode list
    DetailView.swift        inspector for the selected file
```
