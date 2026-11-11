// player.js (interface) — le lecteur intégré : l'écran devient le film, comme
// sur Mac. Utilisé quand mpv n'est pas installé (ou si on le choisit dans les
// réglages). Il lit ce que Chromium sait décoder (MP4, WebM, MKV en H.264/VP9/
// AV1 avec AAC/Opus) ; pour le reste, « Ouvrir dans un lecteur externe ».
//
// Commandes : Espace lecture/pause · ←/→ 10 s · ↑/↓ volume · molette volume ·
// B piste audio suivante · F ou double-clic plein écran · Échap quitte le plein
// écran, sinon revient à la bibliothèque · clic pause.
(function () {
  'use strict';
  const api = window.api;
  const { esc } = window.UI;
  const fmt = (s) => window.Shared.formatTime(s);

  let current = null;

  function fileURL(p) { return 'file://' + p.split('/').map(encodeURIComponent).join('/'); }

  function open(queue, start) {
    close();
    const host = document.getElementById('playerHost');
    const el = document.createElement('div');
    el.className = 'player';
    el.innerHTML = `<video preload="auto"></video>
      <div class="chrome">
        <div class="top"><button class="circle" data-p="close" title="Retour (Échap)" style="pointer-events:auto">${icon('chevronLeft', 18)}</button>
          <div><div class="t"></div><div class="s"></div></div></div>
        <div class="bottom">
          <input class="seek" type="range" min="0" max="1000" value="0">
          <div class="ctl">
            <button data-p="toggle" title="Lecture/pause (Espace)"></button>
            <button data-p="back10" title="Reculer de 10 s (←)">−10</button>
            <button data-p="fwd10" title="Avancer de 10 s (→)">+10</button>
            <span class="time">0:00 / 0:00</span>
            <span style="flex:1"></span>
            <button data-p="audio" title="Piste audio suivante (B)">${icon('music', 18)}</button>
            <button data-p="next" title="Épisode suivant">${icon('chevronRight', 18)}</button>
            <span>${icon('volume', 18)}</span><input class="vol" type="range" min="0" max="100" value="100" style="width:90px">
            <button data-p="external" title="Ouvrir dans un lecteur externe">${icon('external', 18)}</button>
            <button data-p="fs" title="Plein écran (F)">${icon('grid', 16)}</button>
          </div>
        </div>
      </div>`;
    host.appendChild(el);
    const video = el.querySelector('video');
    let saved = { vol: 1 };
    try { saved.vol = Number(localStorage.getItem('volume') || 1); } catch { /* sans stockage */ }
    video.volume = Math.min(Math.max(saved.vol, 0), 1);
    el.querySelector('.vol').value = Math.round(video.volume * 100);

    const s = { el, video, queue, index: 0, played: 0, lastT: 0, lastSaved: -1, saveTimer: null, idleTimer: null, startTimer: null, startAt: start || 0, started: false };
    current = s;
    load(s, 0, start || 0);

    s.saveTimer = setInterval(() => save(s, true), 2000);
    video.addEventListener('timeupdate', () => {
      const t = video.currentTime;
      const step = t - s.lastT;
      if (step > 0 && step < 2.5) s.played += step;
      s.lastT = t;
      updateChrome(s);
    });
    video.addEventListener('playing', () => { s.started = true; clearTimeout(s.startTimer); api.call('inhibit', true); updateChrome(s); poke(s); });
    video.addEventListener('pause', () => { api.call('inhibit', false); save(s, false); updateChrome(s); poke(s); });
    video.addEventListener('ended', () => {
      if (video.duration > 0 && s.played > 3) api.call('watchUpdate', s.queue[s.index].id, video.duration, video.duration, false);
      if (s.index + 1 < s.queue.length) load(s, s.index + 1, 0); else close();
    });
    video.addEventListener('error', () => failed(s));
    video.addEventListener('loadedmetadata', () => { if (s.startAt > 0 && s.startAt < video.duration) video.currentTime = s.startAt; s.startAt = 0; updateChrome(s); });

    el.addEventListener('mousemove', () => poke(s));
    el.addEventListener('click', (e) => {
      const b = e.target.closest('[data-p]');
      if (b) { action(s, b.dataset.p); return; }
      if (e.target === video) { clearTimeout(s.clickTimer); s.clickTimer = setTimeout(() => toggle(s), 220); }
    });
    el.addEventListener('dblclick', (e) => { if (e.target === video) { clearTimeout(s.clickTimer); fullscreen(); } });
    el.addEventListener('wheel', (e) => { e.preventDefault(); volume(s, e.deltaY < 0 ? 0.05 : -0.05); }, { passive: false });
    el.querySelector('.seek').addEventListener('input', (e) => { if (video.duration) video.currentTime = video.duration * e.target.value / 1000; });
    el.querySelector('.vol').addEventListener('input', (e) => { video.volume = e.target.value / 100; rememberVolume(video.volume); });
    s.onKey = (e) => onKey(s, e);
    document.addEventListener('keydown', s.onKey, true);
  }

  function load(s, index, start) {
    if (s.index !== index) save(s, false);
    s.index = index; s.played = 0; s.lastT = start; s.lastSaved = -1; s.startAt = start; s.started = false;
    const item = s.queue[index];
    s.el.querySelector('.top .t').textContent = item.title;
    s.el.querySelector('.top .s').textContent = item.subtitle || '';
    s.el.querySelector('[data-p="next"]').style.display = index + 1 < s.queue.length ? '' : 'none';
    const err = s.el.querySelector('.err'); if (err) err.remove();
    s.video.src = fileURL(item.path);
    s.video.play().catch(() => {});
    // Un fichier qui ne démarre pas au bout de 15 s : une erreur et une porte de sortie,
    // plutôt qu'une « fin » instantanée qui marquerait toute la saison comme vue.
    clearTimeout(s.startTimer);
    s.startTimer = setTimeout(() => { if (!s.started) failed(s); }, 15000);
    poke(s);
  }

  function failed(s) {
    if (s.el.querySelector('.err')) return;
    clearTimeout(s.startTimer);
    const item = s.queue[s.index];
    const box = document.createElement('div');
    box.className = 'err';
    box.innerHTML = `<div>${icon('warning', 34)}<div style="font-size:16px;font-weight:600">Ce fichier ne peut pas être lu ici</div>
      <div style="color:rgba(255,255,255,.7)">Le lecteur intégré ne connaît pas ce format (${esc(item.ext.toUpperCase())}). Installez mpv pour tout lire dans Biblifilm :<br><code>flatpak install flathub io.mpv.Mpv</code></div>
      <div class="hstack"><button class="btn primary" data-p="external">Ouvrir dans un lecteur externe</button><button class="btn" data-p="close">Retour</button></div></div>`;
    s.el.appendChild(box);
  }

  function save(s, routine) {
    const v = s.video, item = s.queue[s.index];
    if (!item || s.played <= 3 || !(v.duration > 0)) return;
    const p = Math.round(v.currentTime * 10) / 10;
    if (routine && p === s.lastSaved) return;
    s.lastSaved = p;
    api.call('watchUpdate', item.id, v.currentTime, v.duration, routine);
  }

  function updateChrome(s) {
    const v = s.video;
    s.el.querySelector('[data-p="toggle"]').innerHTML = icon(v.paused ? 'play' : 'pause', 22);
    s.el.querySelector('.time').textContent = `${fmt(v.currentTime)} / ${fmt(v.duration || 0)}`;
    if (v.duration) s.el.querySelector('.seek').value = Math.round(v.currentTime / v.duration * 1000);
  }

  /** Les commandes s'effacent après 3 s de lecture, et le pointeur avec elles. */
  function poke(s) {
    s.el.classList.remove('idle');
    clearTimeout(s.idleTimer);
    s.idleTimer = setTimeout(() => { if (!s.video.paused) s.el.classList.add('idle'); }, 3000);
  }

  function osd(s, text) {
    let o = s.el.querySelector('.osd');
    if (!o) { o = document.createElement('div'); o.className = 'osd'; s.el.appendChild(o); }
    o.textContent = text;
    clearTimeout(s.osdTimer);
    s.osdTimer = setTimeout(() => o.remove(), 900);
  }

  function rememberVolume(v) { try { localStorage.setItem('volume', String(v)); } catch { /* sans stockage */ } }

  function volume(s, delta) {
    s.video.volume = Math.min(1, Math.max(0, Math.round((s.video.volume + delta) * 100) / 100));
    s.el.querySelector('.vol').value = Math.round(s.video.volume * 100);
    rememberVolume(s.video.volume);
    osd(s, `Volume ${Math.round(s.video.volume * 100)} %`);
  }

  function toggle(s) { if (s.video.paused) s.video.play().catch(() => {}); else s.video.pause(); }

  function fullscreen() {
    if (document.fullscreenElement) document.exitFullscreen(); else document.documentElement.requestFullscreen();
  }

  function nextAudio(s) {
    const tracks = s.video.audioTracks;
    if (!tracks || tracks.length < 2) { osd(s, 'Une seule piste audio'); return; }
    let cur = 0;
    for (let i = 0; i < tracks.length; i++) if (tracks[i].enabled) cur = i;
    const next = (cur + 1) % tracks.length;
    for (let i = 0; i < tracks.length; i++) tracks[i].enabled = i === next;
    const t = tracks[next];
    osd(s, `Audio : ${t.label || t.language || 'piste ' + (next + 1)}`);
  }

  function action(s, a) {
    switch (a) {
      case 'close': close(); break;
      case 'toggle': toggle(s); break;
      case 'back10': s.video.currentTime = Math.max(0, s.video.currentTime - 10); break;
      case 'fwd10': s.video.currentTime = Math.min(s.video.duration || 0, s.video.currentTime + 10); break;
      case 'audio': nextAudio(s); break;
      case 'next': if (s.index + 1 < s.queue.length) load(s, s.index + 1, 0); break;
      case 'fs': fullscreen(); break;
      case 'external': { const id = s.queue[s.index].id; close(); api.call('openExternally', [id]); break; }
      default:
    }
  }

  function onKey(s, e) {
    if (e.ctrlKey || e.altKey || e.metaKey) return;
    let handled = true;
    switch (e.key) {
      case ' ': toggle(s); break;
      case 'ArrowLeft': action(s, 'back10'); osd(s, '−10 s'); break;
      case 'ArrowRight': action(s, 'fwd10'); osd(s, '+10 s'); break;
      case 'ArrowUp': volume(s, 0.05); break;
      case 'ArrowDown': volume(s, -0.05); break;
      case 'b': case 'B': nextAudio(s); break;
      case 'f': case 'F': fullscreen(); break;
      // Échap suit la logique du système : d'abord quitter le plein écran.
      case 'Escape': if (document.fullscreenElement) document.exitFullscreen(); else close(); break;
      default: handled = false;
    }
    if (handled) { e.preventDefault(); e.stopPropagation(); poke(s); }
  }

  function close() {
    const s = current;
    if (!s) return;
    current = null;
    save(s, false);
    clearInterval(s.saveTimer); clearTimeout(s.startTimer); clearTimeout(s.idleTimer);
    document.removeEventListener('keydown', s.onKey, true);
    s.video.pause();
    s.video.removeAttribute('src');
    s.video.load();
    s.el.remove();
    api.call('inhibit', false);
    api.call('saveWatch');
    if (document.fullscreenElement) document.exitFullscreen();
  }

  window.addEventListener('beforeunload', () => { if (current) save(current, false); });
  window.BuiltinPlayer = { open, close, isOpen: () => !!current };
}());
