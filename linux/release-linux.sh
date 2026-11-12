#!/bin/bash
# Construit l'AppImage Linux et l'ajoute à une release GitHub déjà publiée,
# à côté du zip macOS (qui n'est pas touché).
#
#   ./release-linux.sh            version lue dans Cargo.toml
#   ./release-linux.sh 1.4.0      version donnée : Cargo.toml est mis à jour
#
# La release vX.Y.Z doit exister (./release.sh sur main la crée avec la version
# Mac). Le fichier ajouté s'appelle Biblifilm-X.Y.Z-linux-x86_64.AppImage : c'est
# celui que la mise à jour intégrée de la version Linux cherche, et que la
# version Mac ignore. Une AppImage déjà présente sous ce nom est remplacée.
#
# À lancer là où build-appimage.sh fonctionne (Fedora, ou le conteneur toolbox
# décrit dans README.md) ; gh doit être connecté.
set -euo pipefail
cd "$(dirname "$0")"

NAME="Biblifilm"
REPO="VGioM/Biblifilm"

VERSION="${1:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)}"
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "usage : ./release-linux.sh [X.Y.Z]"; exit 1; }
TAG="v$VERSION"
FILE="dist/$NAME-$VERSION-linux-x86_64.AppImage"

gh release view "$TAG" -R "$REPO" >/dev/null 2>&1 \
    || { echo "✗ La release $TAG n'existe pas : publiez d'abord la version Mac (./release.sh sur main)."; exit 1; }

echo "▸ Version $VERSION…"
sed -i "0,/^version = \".*\"/s//version = \"$VERSION\"/" Cargo.toml

rm -rf dist
./build-appimage.sh
[ -f "$FILE" ] || { echo "✗ $FILE introuvable."; exit 1; }

echo "▸ Ajout à la release $TAG…"
gh release upload "$TAG" "$FILE" -R "$REPO" --clobber

# La section Linux des notes : remplacée si elle existe, ajoutée sinon.
NOTES="$(gh release view "$TAG" -R "$REPO" --json body -q .body)"
TMP_NOTES="$(mktemp)"
{
    awk '/^## Linux/{skip=1; next} skip && /^## /{skip=0} !skip' <<<"$NOTES" | sed -e :a -e '/^\n*$/{$d;N;ba' -e '}'
    printf '\n'
    cat <<DOC
## Linux

Téléchargez \`$NAME-$VERSION-linux-x86_64.AppImage\`, rendez-le exécutable (\`chmod +x\`, ou Propriétés → Autoriser l'exécution), puis lancez-le. Application native (Rust), sans Electron. Elle vérifie elle-même au lancement s'il existe une nouvelle version et remplace son AppImage (menu ··· → Rechercher les mises à jour…).

La vidéo est lue dans la fenêtre par libmpv, inclus dans l'AppImage : plus besoin d'installer mpv.

**Configuration requise :** Linux x86_64 (Fedora, Bazzite, Ubuntu, Arch…).
DOC
} > "$TMP_NOTES"
gh release edit "$TAG" -R "$REPO" --notes-file "$TMP_NOTES"
rm -f "$TMP_NOTES"
echo "✓ $FILE ajouté à $NAME $VERSION"
