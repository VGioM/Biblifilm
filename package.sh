#!/bin/bash
# Packages the project into a single archive you can carry to another machine.
#
#   ./package.sh                  sources only
#   ./package.sh --with-data      + your settings, watch history and caches
#   ./package.sh --with-data --with-key   + the TMDB API key
#
# VLCKit (~84 MB) is never included: ./build.sh re-downloads it.
set -euo pipefail
cd "$(dirname "$0")"

WITH_DATA=0
WITH_KEY=0
for arg in "$@"; do
    case "$arg" in
        --with-data) WITH_DATA=1 ;;
        --with-key)  WITH_KEY=1 ;;
        -h|--help)   sed -n '2,9p' "$0"; exit 0 ;;
        *) echo "option inconnue : $arg"; exit 1 ;;
    esac
done

STAMP="$(date +%Y%m%d-%H%M)"
NAME="Biblifilm-$STAMP"
OUT="$HOME/Desktop/$NAME.tar.gz"
STAGE="$(mktemp -d)/$NAME"
trap 'rm -rf "$(dirname "$STAGE")"' EXIT
mkdir -p "$STAGE"

echo "▸ Copie des sources…"
cp -R Sources Package.swift README.md "$STAGE/"
[ -d Resources ] && cp -R Resources "$STAGE/"
cp build.sh fetch-vlckit.sh install.sh package.sh "$STAGE/"
[ -f .gitignore ] && cp .gitignore "$STAGE/"

SUPPORT="$HOME/Library/Application Support/Biblifilm"
if [ "$WITH_DATA" = "1" ]; then
    echo "▸ Ajout des données personnelles…"
    mkdir -p "$STAGE/data"
    [ -d "$SUPPORT" ] && cp -R "$SUPPORT/." "$STAGE/data/" 2>/dev/null || true

    # Export only the settings that matter; window positions are noise.
    KEY=""
    [ "$WITH_KEY" = "1" ] && KEY="$(defaults read com.biblifilm.app tmdbAPIKey 2>/dev/null || echo '')"
    cat > "$STAGE/data/settings.json" <<JSON
{
  "libraryPath": "$(defaults read com.biblifilm.app libraryPath 2>/dev/null || echo '')",
  "preferredLanguage": "$(defaults read com.biblifilm.app preferredLanguage 2>/dev/null || echo 'fr-FR')",
  "tmdbAPIKey": "$KEY"
}
JSON
    [ "$WITH_KEY" = "1" ] || echo "  (clé TMDB non incluse — utilisez --with-key pour l'ajouter)"
fi

cat > "$STAGE/RESTORE.md" <<'DOC'
# Reprendre Biblifilm sur une autre machine

## Ce qu'il faut sur la nouvelle machine
- macOS 14 ou plus récent
- Xcode ou les outils en ligne de commande (`xcode-select --install`)
- VLC (facultatif : sert seulement de secours « Ouvrir dans VLC »)

## Remise en route

```bash
tar -xzf Biblifilm-*.tar.gz
cd Biblifilm-*
./build.sh          # télécharge VLCKit (~84 Mo) puis compile
./install.sh        # facultatif : installe dans /Applications
open dist/Biblifilm.app
```

## Restaurer vos données

Si l'archive contient un dossier `data/` :

```bash
mkdir -p ~/Library/Application\ Support/Biblifilm
cp data/watch-state.json data/metadata-cache.json ~/Library/Application\ Support/Biblifilm/
cp -R data/Artwork ~/Library/Application\ Support/Biblifilm/ 2>/dev/null

# Réglages (adaptez le chemin de la bibliothèque à la nouvelle machine)
python3 - <<'PY'
import json, subprocess
s = json.load(open("data/settings.json"))
for k, v in s.items():
    if v:
        subprocess.run(["defaults", "write", "com.biblifilm.app", k, "-string", v])
print("réglages restaurés")
PY
```

`watch-state.json` contient les épisodes vus et les positions de reprise —
c'est le fichier à ne pas perdre. Une sauvegarde de la version précédente est
gardée à côté sous `watch-state.backup.json`, et l'app peut la restaurer depuis
Réglages → Métadonnées.

Si la clé TMDB n'est pas dans `settings.json`, remettez-la dans
Réglages → Métadonnées, puis « Tester la clé » : les recherches manquantes
repartent toutes seules.

## Reprendre le travail avec Claude

Ouvrez le dossier décompressé et lancez Claude Code dedans. `README.md` décrit
l'architecture, les choix techniques et les pièges déjà rencontrés.
DOC

echo "▸ Compression…"
tar -czf "$OUT" -C "$(dirname "$STAGE")" "$NAME"

echo "✓ $OUT  ($(du -h "$OUT" | cut -f1))"
[ "$WITH_DATA" = "1" ] && echo "  (contient vos réglages et votre historique)"
exit 0
