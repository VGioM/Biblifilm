#!/bin/bash
# Construit l'AppImage Linux et l'ajoute à une release GitHub déjà publiée,
# à côté du zip macOS (qui n'est pas touché).
#
#   ./release-linux.sh            version lue dans package.json
#   ./release-linux.sh 1.4.0      version donnée : package.json est mis à jour
#
# La release vX.Y.Z doit exister (./release.sh sur main la crée avec la version
# Mac). Le fichier ajouté s'appelle Biblifilm-X.Y.Z-linux-x86_64.AppImage : c'est
# celui que la mise à jour intégrée de la version Linux cherche, et que la
# version Mac ignore.
set -euo pipefail
cd "$(dirname "$0")"

NAME="Biblifilm"
REPO="VGioM/Biblifilm"

VERSION="${1:-$(node -p "require('./package.json').version")}"
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "usage : ./release-linux.sh [X.Y.Z]"; exit 1; }
TAG="v$VERSION"
FILE="dist/$NAME-$VERSION-linux-x86_64.AppImage"

gh release view "$TAG" -R "$REPO" >/dev/null 2>&1 \
    || { echo "✗ La release $TAG n'existe pas : publiez d'abord la version Mac (./release.sh sur main)."; exit 1; }

echo "▸ Version $VERSION…"
npm version --no-git-tag-version --allow-same-version "$VERSION" >/dev/null

echo "▸ Dépendances…"
[ -d node_modules ] || npm install --no-audit --no-fund

echo "▸ Tests…"
npm test >/dev/null || { echo "✗ Les tests échouent."; exit 1; }

echo "▸ AppImage…"
rm -rf dist
npx electron-builder --linux AppImage >/dev/null
[ -f "$FILE" ] || { echo "✗ $FILE introuvable."; exit 1; }

echo "▸ Ajout à la release $TAG…"
gh release upload "$TAG" "$FILE" -R "$REPO" --clobber

# Une ligne d'installation pour Linux dans les notes, une seule fois.
NOTES="$(gh release view "$TAG" -R "$REPO" --json body -q .body)"
if ! grep -q "^## Linux" <<<"$NOTES"; then
    TMP_NOTES="$(mktemp)"
    {
        printf '%s\n\n' "$NOTES"
        cat <<DOC
## Linux

Téléchargez \`$NAME-$VERSION-linux-x86_64.AppImage\`, rendez-le exécutable (\`chmod +x\`, ou Propriétés → Autoriser l'exécution), puis lancez-le. L'app vérifie elle-même au lancement s'il existe une nouvelle version et remplace son AppImage (menu Aide → Rechercher les mises à jour…).

**Configuration requise :** Linux x86_64 (Fedora, Bazzite, Ubuntu, Arch…).
DOC
    } > "$TMP_NOTES"
    gh release edit "$TAG" -R "$REPO" --notes-file "$TMP_NOTES"
    rm -f "$TMP_NOTES"
fi
echo "✓ $FILE ajouté à $NAME $VERSION"
