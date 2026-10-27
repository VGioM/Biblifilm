# Règles pour ce dépôt

## Identité git : ne jamais exposer d'adresse e-mail personnelle

Ce dépôt est public. Tous les commits doivent utiliser l'identité GitHub « noreply » :

```sh
git config user.name  "VGioM"
git config user.email "309474935+VGioM@users.noreply.github.com"
```

- Vérifier `git config user.email` avant de committer (sur un nouveau PC, le régler aussi en `--global`, avec `git config --global user.useConfigOnly true`).
- Avant chaque `git push`, vérifier que `git log --format='%ae %ce'` ne montre que l'adresse noreply.
- Ne jamais écrire d'adresse e-mail personnelle dans un commit ou un fichier du dépôt.
