// app.js — l'interface de Biblifilm : barre latérale, grille d'affiches (un
// film, ou une saison en pile), page d'une saison, panneau d'informations,
// feuille « Corriger », réglages.
(function () {
  'use strict';
  const S = window.Shared;
  const { esc, h, menu, menuFromButton, sheet, bytes } = window.UI;
  const $ = (id) => document.getElementById(id);
  const api = window.api;

  let items = [], records = {}, status = null;
  let W = S.Watch(records);
  const ui = { search: '', openedSeason: null, selected: null, bannerDismissed: false };
  const setting = (k) => status.settings[k];
  const sel = () => setting('lastSelection') || 'all';
  const byId = (id) => items.find((i) => i.id === id);

  // ── Affiches ─────────────────────────────────────────────────────────────

  function art(url, seed, symbol, style) {
    return `<div class="cover" style="background-image:${UI.placeholder(seed)};${style || ''}">
      <div class="ph">${icon(symbol, 26)}</div>${url ? `<img class="art" loading="lazy" src="${esc(url)}" alt="" onerror="this.remove()">` : ''}`;
  }

  function cardHTML(e) {
    const season = e.type === 'season';
    const prog = W.entryProgress(e), done = W.entryWatched(e);
    const kind = S.entryKind(e);
    const seen = season ? W.watchedCount(e) : 0;
    const style = kind === 'audio' ? '--cover-aspect:1' : '';
    let cover = art(e.posterURL, e.seriesTitle, S.KIND[kind].symbol, style);
    cover += `<div class="hover"><button class="circle" data-act="primary" title="${season ? 'Voir les épisodes' : 'Lire'}">${icon(season ? 'list' : 'play', 16)}</button>${season ? `<button class="circle" data-act="playEntry" title="Lire">${icon('play', 16)}</button>` : ''}</div>`;
    if (prog > 0.01 && !done) cover += `<div class="resume"><div style="width:${(prog * 100).toFixed(1)}%"></div></div>`;
    if (done) cover += `<div class="seen">${icon('check', 13)}</div>`;
    if (season) cover += `<span class="badge epcount num">${e.episodes.length}</span>`;
    cover += '</div>';
    return `${season ? `<div class="stackwrap">${cover}</div>` : cover}
      <div class="t1">${esc(e.seriesTitle)}</div>
      <div class="t2">${esc(S.entrySubtitle(e))}${seen > 0 ? ` <span class="watched">· ${seen}/${e.episodes.length} vus</span>` : ''}</div>`;
  }

  // ── Barre latérale ───────────────────────────────────────────────────────

  function renderSidebar() {
    const counts = S.counts(items, W);
    const row = (id, title, ic, count) => `<button class="side-row${sel() === id ? ' active' : ''}" data-sel="${esc(id)}">${icon(ic)}<span class="label">${esc(title)}</span>${count ? `<span class="count">${count}</span>` : ''}</button>`;
    let html = '<h3>Bibliothèque</h3>';
    for (const s of S.SECTIONS) {
      if (s.id === 'continueWatching' && !counts.continueWatching) continue;
      if (s.id === 'audio' && !setting('includeAudio')) continue;
      if (s.id === 'unmatched' && !counts.unmatched) continue;
      html += row(s.id, s.title, s.icon, counts[s.id]);
    }
    const series = S.seriesNames(items);
    if (series.length) {
      const open = setting('seriesExpanded');
      html += `<h3 class="fold" data-act="foldSeries">${icon(open ? 'chevronDown' : 'chevronRight', 12)}Séries</h3>`;
      if (open) html += series.map((n) => row('series:' + n, n, 'stack', 0)).join('');
    }
    $('sidebar').innerHTML = html;
    $('sidefoot').innerHTML = `<button class="link" data-act="chooseFolder">${icon('folderPlus')}${status.settings.libraryPath ? 'Changer de dossier…' : 'Choisir un dossier…'}</button>
      <button class="link" data-act="settings">${icon('gear')}Réglages…</button>`;
  }

  // ── Barre d'outils ───────────────────────────────────────────────────────

  function openedSeason() {
    if (!ui.openedSeason) return null;
    return S.build(items).find((e) => e.type === 'season' && e.id === ui.openedSeason) || null;
  }

  function renderToolbar(entries) {
    const season = openedSeason();
    $('back').classList.toggle('hidden', !season);
    $('title').textContent = season ? season.seriesTitle : S.sectionTitle(sel());
    let sub = '';
    if (status.isScanning) sub = status.scanProgress;
    else if (status.isFetching) sub = `Recherche des infos… ${Math.round(status.metadataProgress * 100)} %`;
    else if (status.mpvInstalling) sub = status.mpvInstall;
    else if (season) sub = season.subtitle;
    else if (entries.length) sub = `${entries.length} élément${entries.length > 1 ? 's' : ''}`;
    $('subtitle').textContent = sub;
    $('progress').innerHTML = status.isFetching ? '<span class="spinner"></span>' : '';
    $('playAll').disabled = !entries.length;
  }

  function renderBanner() {
    // Sans clé TMDB, les films ne peuvent pas être identifiés : le dire franchement.
    const unmatched = items.filter((i) => !S.matched(i)).length;
    if (status.hasTMDBKey || !unmatched || ui.bannerDismissed || !status.settings.libraryPath || openedSeason()) { $('banner').innerHTML = ''; return; }
    $('banner').innerHTML = `<div class="keybanner">${icon('key', 16)}<div style="flex:1"><div class="t">${unmatched} fichiers non identifiés</div>
      <div class="s">Sans clé TMDB, les films ne peuvent pas être identifiés — seules les séries le sont.</div></div>
      <button class="btn primary" data-act="settingsMeta">Ajouter une clé…</button><button class="btn plain icon" data-act="dismissBanner">${icon('x', 12)}</button></div>`;
  }

  // ── Contenu ──────────────────────────────────────────────────────────────

  function emptyState(ic, title, message, action, act) {
    return `<div class="empty"><div class="big">${icon(ic, 46)}</div><div class="t">${esc(title)}</div><div class="m">${esc(message)}</div>
      ${action ? `<button class="btn primary large" data-act="${act}">${esc(action)}</button>` : ''}</div>`;
  }

  let mode = null;
  function renderContent(entries) {
    const c = $('content');
    let want;
    if (!status.settings.libraryPath) want = 'nofolder';
    else if (status.isScanning && !items.length) want = 'scanning';
    else if (openedSeason()) want = 'season';
    else if (!entries.length) want = 'empty';
    else want = 'grid';

    if (want === 'grid') {
      if (mode !== 'grid') { c.innerHTML = '<div class="grid" id="items"></div>'; mode = 'grid'; }
      UI.patchList($('items'), entries, S.entryId, cardHTML, (e) => `card${e.type === 'season' ? ' season' : ''}${e.type === 'single' && ui.selected === e.item.id ? ' selected' : ''}`);
      return;
    }
    mode = want;
    if (want === 'nofolder') c.innerHTML = emptyState('folder', 'Aucun dossier sélectionné', "Choisissez le dossier qui contient vos films et séries. Biblifilm l'analysera et retrouvera les titres et les jaquettes.", 'Choisir un dossier…', 'chooseFolder');
    else if (want === 'scanning') c.innerHTML = `<div class="empty"><span class="spinner large"></span><div class="m">${esc(status.scanProgress || 'Analyse…')}</div></div>`;
    else if (want === 'empty') c.innerHTML = ui.search ? emptyState('search', 'Aucun résultat', `Aucun média ne correspond à « ${ui.search} ».`) : emptyState('film', 'Rien à afficher ici', 'Aucun média ne correspond à cette catégorie.');
    else renderSeason(openedSeason());
  }

  function seasonButtonTitle(g) {
    const next = W.nextEpisode(g);
    if (next && W.resume(next) != null) return `Reprendre l'épisode ${S.episodeNumber(next) || 1}`;
    if (W.watchedCount(g) > 0 && next) return `Lire l'épisode ${S.episodeNumber(next) || 1}`;
    return 'Lire la saison';
  }

  function episodeHTML(ep, i) {
    const done = W.watched(ep), r = W.rec(ep), resume = W.resume(ep);
    const left = r ? Math.max(r.duration - r.position, 0) / 60 : 0;
    const remaining = left >= 60 ? `${Math.floor(left / 60)} h ${Math.floor(left % 60)} min` : `${Math.floor(left)} min`;
    return `<div class="num"><span class="n">${done ? icon('check', 12, 'done') : (S.episodeNumber(ep) || i + 1)}</span><button class="play" data-act="playEp">${icon('play', 12)}</button></div>
      <div class="titles"><div class="t">${esc(S.episodeRowTitle(ep))}</div>
      ${resume != null ? `<div class="r" title="Reprendre à ${S.formatTime(r.position)}"><span class="bar"><div style="width:${(S.progressOf(r) * 100).toFixed(1)}%"></div></span>reste ${remaining}<span class="tertiary">· ${S.formatTime(r.position)}</span></div>`
        : `<div class="f">${esc(ep.filename)}</div>`}</div>
      <div class="size">${bytes(ep.size)}</div>`;
  }

  function renderSeason(g) {
    const c = $('content');
    if (!c.querySelector('.season-page') || c.dataset.season !== g.id) {
      c.dataset.season = g.id;
      c.innerHTML = '<div class="season-page"><div class="season-head" id="seasonHead"></div><div class="ep-sep" style="margin-left:0"></div><div class="eplist" id="eplist"></div></div>';
      c.scrollTop = 0;
    }
    $('seasonHead').innerHTML = `${art(g.posterURL, g.seriesTitle, 'tv')}</div>
      <div class="stack" style="gap:7px;min-width:0">
        <h1>${esc(g.seriesTitle)}</h1><div class="sub">${esc(g.subtitle)}</div>
        <div class="muted">${[g.year, bytes(g.totalSize)].filter(Boolean).join(' · ')}</div>
        ${g.genres.length ? `<div class="chips">${g.genres.slice(0, 3).map((x) => `<span class="chip">${esc(x)}</span>`).join('')}</div>` : ''}
        ${g.overview ? `<div class="ov">${esc(g.overview)}</div>` : ''}
        <div class="hstack" style="padding-top:4px"><button class="btn primary large" data-act="playSeason">${icon('play', 13)}${esc(seasonButtonTitle(g))}</button>
          <button class="btn plain icon" data-act="seasonMenu" title="Plus">${icon('more', 18)}</button></div>
        ${g.isIdentified ? '' : '<div class="muted" style="font-size:11.5px">Série non identifiée — ajoutez une clé TMDB dans les réglages, ou utilisez « Corriger » sur un épisode.</div>'}
      </div>`;
    const list = $('eplist');
    const rows = [];
    g.episodes.forEach((ep, i) => { rows.push({ key: ep.id, ep, i }); if (i < g.episodes.length - 1) rows.push({ key: 'sep:' + ep.id, sep: true }); });
    UI.patchList(list, rows, (r) => r.key, (r) => (r.sep ? '' : episodeHTML(r.ep, r.i)),
      (r) => (r.sep ? 'ep-sep' : `ep${ui.selected === r.ep.id ? ' selected' : ''}${W.watched(r.ep) ? ' seen' : ''}`));
  }

  // ── Panneau d'informations ───────────────────────────────────────────────

  function inspectorHTML(it) {
    const m = S.matched(it) ? it.metadata : null;
    const r = W.rec(it), resume = W.resume(it);
    const still = m && m.stillURL;
    const aspect = still ? '16/9' : it.parsed.kind === 'audio' ? '1' : '2/3';
    const row = (k, v) => `<div class="row"><div class="k">${esc(k)}</div><div class="v">${esc(v)}</div></div>`;
    const player = status.players.mpv && setting('player') !== 'builtin' ? 'mpv' : 'le lecteur intégré';
    return `<div class="insp">
      <div style="box-shadow:0 5px 24px rgba(0,0,0,.45);border-radius:10px">${art(still || (m && m.posterURL), S.groupingTitle(it), S.KIND[it.parsed.kind].symbol, `--cover-aspect:${aspect};border-radius:10px`)}</div></div>
      <div><h2>${esc(S.displayTitle(it))}</h2>
        ${m && m.seriesTitle && it.parsed.kind === 'episode' ? `<div style="color:var(--accent)">${esc(m.seriesTitle)}</div>` : ''}
        <div class="muted">${esc(S.displaySubtitle(it))}</div></div>
      <div class="stack">
        <button class="btn primary large block" data-act="play" title="Lire avec ${player}">${icon('play', 13)}${resume != null ? `Reprendre à ${S.formatTime(r.position)}` : 'Lire'}</button>
        ${resume != null ? `<button class="btn block" data-act="playFromStart">${icon('restart', 13)}Reprendre au début</button>` : ''}
        <button class="btn block" data-act="external">${icon('external', 14)}Ouvrir dans ${status.players.vlc ? 'VLC' : 'un lecteur externe'}</button>
        <button class="btn block" data-act="toggleWatched">${icon(W.watched(it) ? 'eyeOff' : 'checkCircle', 14)}${W.watched(it) ? 'Marquer comme non vu' : 'Marquer comme vu'}</button>
        <div class="hstack"><button class="btn grow" data-act="reveal">${icon('folder', 14)}Dossier</button><button class="btn grow" data-act="correct">${icon('wand', 14)}Corriger</button></div>
      </div>
      ${m && m.overview ? `<div><div class="sec-title">Synopsis</div><div class="selectable" style="font-size:12.5px">${esc(m.overview)}</div></div>` : ''}
      ${m && m.genres && m.genres.length ? `<div class="chips">${m.genres.slice(0, 3).map((g) => `<span class="chip">${esc(g)}</span>`).join('')}</div>` : ''}
      <div style="height:1px;background:var(--hairline)"></div>
      <div><div class="sec-title">Fichier</div>
        ${row('Nom', it.filename)}${row('Type', S.KIND[it.parsed.kind].name)}${row('Taille', bytes(it.size))}${row('Format', it.ext.toUpperCase())}
        ${m && m.rating > 0 ? row('Note', `${m.rating.toFixed(1).replace('.', ',')}/10`) : ''}${m && m.source ? row('Source', m.source) : ''}
        ${it.parsed.releaseGroup ? row('Groupe', it.parsed.releaseGroup) : ''}${row('Chemin', it.path)}</div>
      ${S.matched(it) ? '' : `<div class="notice">${icon('info', 15)}<div><b>Titre non identifié</b><div class="muted">${status.hasTMDBKey ? 'Aucune correspondance trouvée. Essayez « Corriger » pour choisir le bon titre.' : 'Ajoutez une clé TMDB dans les réglages pour de bien meilleurs résultats.'}</div></div></div>`}
    </div>`;
  }

  let lastInsp = null;
  function renderInspector() {
    const it = ui.selected && byId(ui.selected);
    const insp = $('inspector');
    if (!it || !setting('inspectorVisible')) { insp.classList.add('hidden'); lastInsp = null; return; }
    insp.classList.remove('hidden');
    const html = inspectorHTML(it);
    if (html !== lastInsp) { insp.innerHTML = html; lastInsp = html; }
  }

  function render() {
    if (!status) return;
    W = S.Watch(records);
    if (ui.selected && !byId(ui.selected)) ui.selected = null;
    const entries = openedSeason() ? [] : S.visibleEntries(items, sel(), ui.search, setting('sortOrder'), W);
    renderSidebar();
    renderToolbar(entries);
    renderBanner();
    renderContent(entries);
    renderInspector();
    if (status.errorMessage && !render.errorShown) {
      render.errorShown = true;
      sheet((box, close) => {
        box.innerHTML = `<h3>Erreur</h3><div class="selectable" style="white-space:pre-wrap">${esc(status.errorMessage)}</div><div class="hstack"><span class="grow"></span><button class="btn primary">OK</button></div>`;
        box.querySelector('button').onclick = () => { close(); };
      });
      api.call('clearError').then(() => { render.errorShown = false; });
    }
  }

  // ── Actions ──────────────────────────────────────────────────────────────

  const setSel = (s) => { ui.openedSeason = null; ui.selected = null; api.call('setSetting', 'lastSelection', s); status.settings.lastSelection = s; render(); };
  const entryOf = (node) => {
    const key = node && node.dataset.key;
    return key ? S.build(items).find((e) => S.entryId(e) === key) || null : null;
  };

  function openSeason(g) { ui.openedSeason = g.id; ui.selected = g.episodes[0] && g.episodes[0].id; render(); }
  function activate(e) { if (e.type === 'season') openSeason(e); else api.call('play', e.item.id); }

  function itemMenu(it) {
    return [
      { label: 'Lire', action: () => api.call('play', it.id) },
      W.resume(it) != null ? { label: 'Lire depuis le début', action: () => api.call('play', it.id, true) } : null,
      { label: W.watched(it) ? 'Marquer comme non vu' : 'Marquer comme vu', action: () => api.call('setWatched', [it.id], !W.watched(it)) },
      '-',
      { label: `Ouvrir dans ${status.players.vlc ? 'VLC' : 'un lecteur externe'}`, action: () => api.call('openExternally', [it.id]) },
      { label: 'Afficher dans le dossier', action: () => api.call('reveal', it.id) },
      '-',
      { label: 'Rechercher les infos à nouveau', action: () => api.call('refetch', it.id) },
      { label: 'Corriger le titre…', action: () => correct(it) },
      { label: 'Copier le chemin', action: () => api.call('copyPath', it.id) },
    ];
  }

  function seasonMenu(g) {
    return [
      { label: 'Voir les épisodes', action: () => openSeason(g) },
      { label: 'Lire la saison', action: () => api.call('playEntry', S.entryId(g)) },
      '-',
      { label: 'Marquer la saison comme vue', action: () => api.call('setWatched', g.episodes.map((e) => e.id), true) },
      { label: 'Marquer comme non vue', action: () => api.call('setWatched', g.episodes.map((e) => e.id), false) },
      '-',
      { label: 'Rechercher les infos à nouveau', action: () => api.call('refetchMany', g.episodes.map((e) => e.id)) },
    ];
  }

  document.addEventListener('click', (ev) => {
    const a = ev.target.closest('[data-act]');
    const card = ev.target.closest('#items > [data-key]');
    const ep = ev.target.closest('#eplist > .ep');
    if (a) {
      const act = a.dataset.act;
      const e = card ? entryOf(card) : null;
      const it = ep ? byId(ep.dataset.key) : byId(ui.selected);
      const g = openedSeason();
      switch (act) {
        case 'primary': if (e) activate(e); break;
        case 'playEntry': if (e) api.call('playEntry', S.entryId(e)); break;
        case 'playEp': if (it) api.call('play', it.id); break;
        case 'play': if (it) api.call('play', it.id); break;
        case 'playFromStart': if (it) api.call('play', it.id, true); break;
        case 'external': if (it) api.call('openExternally', [it.id]); break;
        case 'toggleWatched': if (it) api.call('setWatched', [it.id], !W.watched(it)); break;
        case 'reveal': if (it) api.call('reveal', it.id); break;
        case 'correct': if (it) correct(it); break;
        case 'playSeason': if (g) api.call('playEntry', S.entryId(g)); break;
        case 'seasonMenu': if (g) menuFromButton(a, seasonMenu(g).slice(1)); break;
        case 'chooseFolder': api.call('chooseFolder'); break;
        case 'settings': showSettings('general'); break;
        case 'settingsMeta': showSettings('metadata'); break;
        case 'dismissBanner': ui.bannerDismissed = true; render(); break;
        case 'foldSeries': api.call('setSetting', 'seriesExpanded', !setting('seriesExpanded')); break;
        default: return;
      }
      ev.stopPropagation();
      return;
    }
    const side = ev.target.closest('[data-sel]');
    if (side) { setSel(side.dataset.sel); return; }
    if (card) {
      const e = entryOf(card);
      if (!e) return;
      if (e.type === 'season') openSeason(e);
      else { ui.selected = e.item.id; if (!setting('inspectorVisible')) api.call('setSetting', 'inspectorVisible', true); render(); }
      return;
    }
    if (ep) { ui.selected = ep.dataset.key; render(); return; }
    // Un clic qui manque les affiches désélectionne, ce qui ferme le panneau.
    if (ev.target.closest('#content') && !openedSeason()) { ui.selected = null; render(); }
  });

  document.addEventListener('dblclick', (ev) => {
    const card = ev.target.closest('#items > [data-key]');
    if (card && !ev.target.closest('[data-act]')) { const e = entryOf(card); if (e) activate(e); return; }
    const ep = ev.target.closest('#eplist > .ep');
    if (ep && !ev.target.closest('[data-act]')) api.call('play', ep.dataset.key);
  });

  document.addEventListener('contextmenu', (ev) => {
    const card = ev.target.closest('#items > [data-key]');
    const ep = ev.target.closest('#eplist > .ep');
    if (card) {
      ev.preventDefault();
      const e = entryOf(card);
      if (e) menu(ev.clientX, ev.clientY, e.type === 'season' ? seasonMenu(e) : itemMenu(e.item));
    } else if (ep) {
      ev.preventDefault();
      const it = byId(ep.dataset.key);
      if (it) menu(ev.clientX, ev.clientY, itemMenu(it));
    }
  });

  document.addEventListener('keydown', (ev) => {
    if (window.BuiltinPlayer.isOpen() || document.querySelector('.sheet-back')) return;
    const typing = document.activeElement && /INPUT|SELECT|TEXTAREA/.test(document.activeElement.tagName);
    if (ev.key === 'Enter' && !typing && ui.selected) { api.call('play', ui.selected); ev.preventDefault(); }
    if (ev.key === 'Escape' && !typing) {
      if (openedSeason()) { ui.openedSeason = null; ui.selected = null; render(); } else if (ui.selected) { ui.selected = null; render(); }
    }
  });

  $('back').innerHTML = icon('chevronLeft', 18);
  $('back').onclick = () => { ui.openedSeason = null; ui.selected = null; render(); };
  $('playAll').innerHTML = icon('playCircle', 19);
  $('playAll').onclick = () => {
    const entries = S.visibleEntries(items, sel(), ui.search, setting('sortOrder'), W);
    api.call('playAll', entries.flatMap((e) => (e.type === 'season' ? e.episodes.map((x) => x.id) : [e.item.id])));
  };
  $('options').innerHTML = icon('more', 19);
  $('options').onclick = () => menuFromButton($('options'), [
    ...S.SORTS.map((s) => ({ label: `Trier par ${s.label.toLowerCase()}`, checked: setting('sortOrder') === s.id, action: () => api.call('setSetting', 'sortOrder', s.id) })),
    '-',
    { label: 'Analyser à nouveau', disabled: !status.settings.libraryPath, action: () => api.call('rescan') },
    { label: 'Rechercher les infos manquantes', disabled: !items.length, action: () => api.call('fetchMetadata') },
    '-',
    { label: 'Réglages…', action: () => showSettings('general') },
  ]);
  $('toggleInspector').innerHTML = icon('sidebar', 18);
  $('toggleInspector').onclick = () => api.call('setSetting', 'inspectorVisible', !setting('inspectorVisible'));
  $('searchIcon').innerHTML = icon('search', 14);
  $('searchClear').innerHTML = icon('x', 12);
  $('search').addEventListener('input', (e) => { ui.search = e.target.value; ui.openedSeason = null; $('searchBox').classList.toggle('has-text', !!ui.search); render(); });
  $('searchClear').addEventListener('click', (e) => { e.preventDefault(); $('search').value = ''; ui.search = ''; $('searchBox').classList.remove('has-text'); render(); });

  // ── Corriger ─────────────────────────────────────────────────────────────

  /** Choisir soi-même le bon titre quand le classement automatique se trompe. */
  function correct(it) {
    const series = it.parsed.kind === 'episode';
    sheet((box, close) => {
      box.innerHTML = `<h3>Corriger le titre</h3>
        <div class="muted" style="font-size:12px">${series ? "Choisissez la bonne série. La correction s'applique à tous les épisodes." : 'Choisissez le bon film dans la liste.'}</div>
        <div class="hstack"><input type="text" class="grow" id="cq" placeholder="Titre"><button class="btn primary" id="cs">Rechercher</button></div>
        <div id="cr" style="min-height:90px"></div>
        <div class="hstack"><button class="btn" id="ca" title="Relance la recherche et garde le meilleur résultat.">Recherche automatique</button><span class="grow"></span><button class="btn" id="cc">Annuler</button></div>`;
      const q = box.querySelector('#cq');
      // Ce que dit le nom du fichier, pas la correspondance affichée : c'est souvent elle qui est fausse.
      q.value = it.parsed.title;
      const results = box.querySelector('#cr');
      const search = async () => {
        if (!status.hasTMDBKey) { results.innerHTML = '<div class="empty" style="position:static;padding:30px"><div class="m">Ajoutez une clé TMDB dans les réglages pour choisir parmi les titres.</div></div>'; return; }
        const typed = q.value.trim();
        if (!typed) return;
        results.innerHTML = '<div class="hstack muted" style="justify-content:center;padding:30px"><span class="spinner"></span>Recherche…</div>';
        const list = await api.call('candidates', it.id, typed);
        if (!list.length) { results.innerHTML = '<div class="muted" style="text-align:center;padding:30px">Aucun résultat. Essayez le titre original, sans l\'année.</div>'; return; }
        results.innerHTML = `<div style="max-height:300px;overflow:auto;border-radius:8px;background:rgba(255,255,255,.05)">${list.map((c, i) => `
          <div class="hstack cand" data-i="${i}" style="align-items:flex-start;padding:8px;cursor:pointer;border-bottom:1px solid var(--hairline)">
            <div style="width:44px;flex:none">${art(c.posterURL, c.title, c.isSeries ? 'tv' : 'film', 'border-radius:4px')}</div></div>
            <div style="min-width:0"><div style="font-weight:500">${esc(c.title)}</div>
            <div style="color:var(--accent);font-size:11.5px">${esc([c.year, c.originalTitle && c.originalTitle !== c.title ? c.originalTitle : null].filter(Boolean).join(' · '))}</div>
            <div class="muted" style="font-size:11px;display:-webkit-box;-webkit-line-clamp:2;-webkit-box-orient:vertical;overflow:hidden">${esc(c.overview || '')}</div></div></div>`).join('')}</div>`;
        results.querySelectorAll('.cand').forEach((n) => {
          n.onmouseenter = () => { n.style.background = 'rgba(255,255,255,.06)'; };
          n.onmouseleave = () => { n.style.background = ''; };
          n.onclick = () => { api.call('applyCandidate', it.id, list[Number(n.dataset.i)], q.value); close(); };
        });
      };
      box.querySelector('#cs').onclick = search;
      q.addEventListener('keydown', (e) => { if (e.key === 'Enter') search(); });
      box.querySelector('#ca').onclick = () => { api.call('refetch', it.id, q.value); close(); };
      box.querySelector('#cc').onclick = close;
      search();
    }, { width: 470 });
  }

  // ── Réglages ─────────────────────────────────────────────────────────────

  const LANGS = [['fr-FR', 'Français'], ['en-US', 'English'], ['es-ES', 'Español'], ['de-DE', 'Deutsch'], ['it-IT', 'Italiano'], ['pt-BR', 'Português'], ['ja-JP', '日本語']];

  function showSettings(tab) {
    sheet((box, close) => {
      let current = tab;
      const draw = async () => {
        const s = status.settings;
        const check = (k, label, help) => `<label class="check"><input type="checkbox" data-k="${k}" ${s[k] ? 'checked' : ''}><span>${esc(label)}${help ? `<div class="help">${esc(help)}</div>` : ''}</span></label>`;
        let body = '';
        if (current === 'general') {
          body = `<div class="group"><div class="gtitle">Dossier</div>
              <div class="hstack"><span class="grow selectable" style="overflow:hidden;text-overflow:ellipsis;white-space:nowrap">${esc(s.libraryPath || 'Aucun')}</span><button class="btn" data-a="choose">Choisir…</button></div>
              ${check('includeAudio', 'Inclure les fichiers audio')}
              ${check('skipSmallFiles', 'Ignorer les fichiers de moins de 20 Mo', "Évite d'importer les extraits, bandes-annonces et échantillons.")}
              ${check('watchFolder', 'Détecter automatiquement les nouveaux fichiers', 'Surveille le dossier en continu : un film ajouté apparaît tout seul, avec sa jaquette.')}
              <div><button class="btn" data-a="rescan" ${s.libraryPath ? '' : 'disabled'}>Analyser à nouveau</button></div></div>
            <div class="group"><div class="gtitle">Mises à jour de Biblifilm</div>
              <label class="check"><input type="checkbox" data-a="auto" ${status.autoUpdate ? 'checked' : ''}><span>Rechercher automatiquement les mises à jour</span></label>
              <div class="hstack"><button class="btn" data-a="checkUpdates">Rechercher maintenant</button><span class="muted">Version ${esc(status.version)}</span></div></div>`;
        } else if (current === 'playback') {
          const mpv = status.players.mpv, vlc = status.players.vlc;
          body = `<div class="group"><div class="gtitle">Lecteur</div>
              <select data-k="player"><option value="auto" ${s.player === 'auto' ? 'selected' : ''}>Automatique (mpv s'il est installé)</option><option value="builtin" ${s.player === 'builtin' ? 'selected' : ''}>Lecteur intégré</option></select>
              <div class="hstack">${icon(mpv ? 'checkCircle' : 'warning', 15)}<span>${mpv ? esc(mpv) + ' : installé' : 'mpv : introuvable'}</span></div>
              <div class="help">${mpv ? 'Avec mpv, tous les formats sont lus (MKV, x265, EAC3, sous-titres) ; la reprise, les épisodes vus et le choix des pistes sont suivis.'
                : 'Sans mpv, le lecteur intégré lit MP4, WebM et la plupart des MKV en H.264. mpv lit tout : Biblifilm l\'installe depuis Flathub, pour votre compte, sans mot de passe.'}</div>
              ${mpv ? '' : status.mpvInstalling ? `<div class="hstack"><span class="spinner"></span><span>${esc(status.mpvInstall)}</span></div>`
                : `<div class="hstack"><button class="btn primary" data-a="installMpv">Installer mpv</button>${status.mpvInstall ? `<span style="color:var(--warm);font-size:12px">${esc(status.mpvInstall)}</span>` : ''}</div>`}
              ${check('fullscreen', 'Ouvrir mpv en plein écran')}
              <div class="hstack">${icon(vlc ? 'checkCircle' : 'info', 15)}<span>${vlc ? esc(vlc) + ' : installé (« Ouvrir dans VLC »)' : "VLC : introuvable — « Ouvrir dans un lecteur externe » utilise l'app par défaut"}</span></div>
              <div><button class="btn" data-a="detect">Détecter</button></div></div>
            <div class="group"><div class="gtitle">Historique de visionnage</div>
              <div class="hstack"><span class="grow">Épisodes suivis</span><span class="muted num">${Object.keys(records).length}</span></div>
              <div class="hstack"><button class="btn" data-a="restore">Restaurer la sauvegarde précédente</button><span class="muted" id="restoreMsg"></span></div></div>`;
        } else {
          const info = await api.call('cacheInfo');
          body = `<div class="group"><div class="gtitle">Clé API TMDB</div>
              <input type="password" data-k="tmdbAPIKey" value="${esc(s.tmdbAPIKey)}" placeholder="Collez votre clé ici">
              <div class="help">Gratuite sur themoviedb.org → Paramètres → API. Sans clé, Biblifilm utilise TVmaze et Wikidata, avec des résultats plus limités.</div>
              <div><button class="linkbtn" data-a="getKey">Obtenir une clé TMDB</button></div>
              <div class="hstack"><button class="btn" data-a="test" ${status.hasTMDBKey ? '' : 'disabled'}>Tester la clé</button><span id="keyStatus" style="font-size:12px"></span></div></div>
            <div class="group"><div class="hstack"><span class="grow">Langue</span><select data-k="preferredLanguage">${LANGS.map(([c, l]) => `<option value="${c}" ${s.preferredLanguage === c ? 'selected' : ''}>${l}</option>`).join('')}</select></div>
              ${check('autoFetchMetadata', "Rechercher automatiquement après l'analyse")}</div>
            <div class="group"><div class="hstack"><button class="btn" data-a="refetchAll" ${items.length ? '' : 'disabled'}>Tout rechercher à nouveau</button><button class="btn" data-a="clearCache">Vider le cache</button></div>
              <div class="help">${info.count} fiches en cache · ${bytes(info.bytes)} d'images</div></div>`;
        }
        box.innerHTML = `<div class="hstack"><h3 class="grow">Réglages</h3><button class="btn plain icon" data-a="close">${icon('x', 14)}</button></div>
          <div class="tabs">${[['general', 'Général'], ['playback', 'Lecture'], ['metadata', 'Métadonnées']].map(([id, l]) => `<button data-tab="${id}" class="${current === id ? 'on' : ''}">${l}</button>`).join('')}</div>${body}`;
        box.querySelectorAll('[data-tab]').forEach((b) => { b.onclick = () => { current = b.dataset.tab; draw(); }; });
        box.querySelectorAll('[data-k]').forEach((inp) => {
          const k = inp.dataset.k;
          const ev = inp.type === 'checkbox' || inp.tagName === 'SELECT' ? 'change' : 'input';
          inp.addEventListener(ev, () => {
            const v = inp.type === 'checkbox' ? inp.checked : inp.value;
            status.settings[k] = v;
            api.call('setSetting', k, v);
            if (k === 'tmdbAPIKey') { const st = box.querySelector('#keyStatus'); if (st) st.textContent = ''; const t = box.querySelector('[data-a="test"]'); if (t) t.disabled = !v.trim(); }
          });
        });
        box.querySelectorAll('[data-a]').forEach((b) => {
          const a = b.dataset.a;
          const handler = async () => {
            switch (a) {
              case 'close': close(); break;
              case 'choose': await api.call('chooseFolder'); setTimeout(draw, 300); break;
              case 'rescan': api.call('rescan'); break;
              case 'auto': api.call('setAutoUpdate', b.checked); break;
              case 'checkUpdates': api.call('checkUpdates'); break;
              case 'detect': await api.call('detectPlayers'); setTimeout(draw, 300); break;
              case 'installMpv': api.call('installMpv').then(() => setTimeout(draw, 300)); setTimeout(draw, 300); break;
              case 'restore': { const n = await api.call('restoreWatch'); box.querySelector('#restoreMsg').textContent = n > 0 ? `${n} entrées restaurées.` : 'Aucune sauvegarde utilisable.'; break; }
              case 'getKey': api.call('openExternal', 'https://www.themoviedb.org/settings/api'); break;
              case 'test': {
                const st = box.querySelector('#keyStatus');
                st.innerHTML = '<span class="spinner"></span>';
                const problem = await api.call('testKey');
                st.innerHTML = problem ? `<span style="color:var(--warm)">${esc(problem)}</span>` : '<span style="color:#3CDC8C">Clé valide</span>';
                break;
              }
              case 'refetchAll': api.call('fetchMetadata', true); break;
              case 'clearCache': await api.call('clearCache'); draw(); break;
              default:
            }
          };
          b.addEventListener(b.type === 'checkbox' ? 'change' : 'click', handler);
        });
      };
      draw();
    }, { width: 560 });
  }

  // ── Messages du processus principal ──────────────────────────────────────

  api.on('state', (msg) => {
    if (msg.items) items = msg.items;
    if (msg.records) records = msg.records;
    if (msg.status) status = msg.status;
    render();
  });
  api.on('playBuiltin', ({ queue, start }) => window.BuiltinPlayer.open(queue, start));
  api.on('focusSearch', () => { $('search').focus(); $('search').select(); });
  api.on('back', () => { ui.openedSeason = null; ui.selected = null; render(); });
  api.on('toggleInspector', () => api.call('setSetting', 'inspectorVisible', !setting('inspectorVisible')));
  api.on('showSettings', () => showSettings('general'));
  api.call('ready');
}());
