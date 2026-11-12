#!/bin/bash
# Construit l'AppImage Linux et la publie dans les releases GitHub, à côté des
# zips macOS (qui ne sont jamais touchés).
#
#   ./release-linux.sh            version lue dans Cargo.toml
#   ./release-linux.sh 1.4.0      version donnée : Cargo.toml est mis à jour
#
# Mac et Linux partagent les releases du dépôt mais pas forcément leurs numéros :
# si la release vX.Y.Z existe (une version Mac du même numéro), l'AppImage y est
# ajoutée ; sinon une release Linux seule est créée. Chaque mise à jour intégrée
# prend la release la plus récente qui contient son propre fichier.
#
# Le fichier ajouté s'appelle Biblifilm-X.Y.Z-linux-x86_64.AppImage : c'est
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

echo "▸ Version $VERSION…"
sed -i "0,/^version = \".*\"/s//version = \"$VERSION\"/" Cargo.toml

rm -rf dist
./build-appimage.sh
[ -f "$FILE" ] || { echo "✗ $FILE introuvable."; exit 1; }

if gh release view "$TAG" -R "$REPO" >/dev/null 2>&1; then
    echo "▸ Ajout à la release $TAG…"
    gh release upload "$TAG" "$FILE" -R "$REPO" --clobber
else
    echo "▸ Release Linux $TAG…"
    git push -q origin HEAD
    gh release create "$TAG" "$FILE" -R "$REPO" --target "$(git rev-parse HEAD)" --title "$NAME $VERSION" \
        --notes "Version Linux uniquement : rien ne change pour la version Mac."
fi

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
