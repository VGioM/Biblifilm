// ui.js — petites briques d'interface communes aux trois apps : échappement,
// mise à jour d'une liste sans tout redessiner, menus contextuels, feuilles
// modales, tailles de fichiers.
(function () {
  'use strict';

  const esc = (s) => String(s == null ? '' : s)
    .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;').replace(/'/g, '&#39;');

  /** Octets → « 1,2 Go », comme le Finder (puissances de 1000). */
  function bytes(n) {
    if (!n || n < 0) return '0 o';
    const u = ['o', 'Ko', 'Mo', 'Go', 'To'];
    let i = 0; let v = n;
    while (v >= 1000 && i < u.length - 1) { v /= 1000; i++; }
    const digits = v >= 100 || i === 0 ? 0 : 1;
    return `${v.toFixed(digits).replace('.', ',')} ${u[i]}`;
  }

  /**
   * Remplit `container` avec un élément par entrée, réutilisés d'un appel à
   * l'autre par leur clé : seul ce qui a changé est redessiné, et le survol,
   * le défilement et les images déjà chargées restent en place.
   */
  function patchList(container, items, keyOf, htmlOf, classOf) {
    const existing = new Map();
    for (const node of Array.from(container.children)) {
      if (node.dataset && node.dataset.key != null) existing.set(node.dataset.key, node);
      else node.remove();
    }
    let prev = null;
    for (const item of items) {
      const key = String(keyOf(item));
      const html = htmlOf(item);
      const cls = classOf ? classOf(item) : '';
      let node = existing.get(key);
      if (node) {
        existing.delete(key);
        if (node._html !== html) { node.innerHTML = html; node._html = html; }
        if (node.className !== cls) node.className = cls;
      } else {
        node = document.createElement('div');
        node.dataset.key = key;
        node.className = cls;
        node.innerHTML = html;
        node._html = html;
      }
      const want = prev ? prev.nextSibling : container.firstChild;
      if (want !== node) container.insertBefore(node, want);
      prev = node;
    }
    for (const node of existing.values()) node.remove();
  }

  // ── Menu contextuel ──────────────────────────────────────────────────────

  let openMenu = null;
  function closeMenu() { if (openMenu) { openMenu.remove(); openMenu = null; } }
  document.addEventListener('mousedown', (e) => { if (openMenu && !openMenu.contains(e.target)) closeMenu(); });
  document.addEventListener('keydown', (e) => { if (e.key === 'Escape' && openMenu) { closeMenu(); e.stopPropagation(); } }, true);
  window.addEventListener('blur', closeMenu);
  window.addEventListener('resize', closeMenu);

  /** items : [{ label, action, disabled, checked } | '-' ] */
  function menu(x, y, items) {
    closeMenu();
    const m = document.createElement('div');
    m.className = 'ctx-menu';
    for (const it of items) {
      if (!it) continue;
      if (it === '-') { m.appendChild(Object.assign(document.createElement('div'), { className: 'ctx-sep' })); continue; }
      const b = document.createElement('button');
      b.className = 'ctx-item' + (it.checked ? ' checked' : '');
      b.textContent = it.label;
      b.disabled = !!it.disabled;
      b.addEventListener('click', () => { closeMenu(); it.action && it.action(); });
      m.appendChild(b);
    }
    document.body.appendChild(m);
    const r = m.getBoundingClientRect();
    m.style.left = Math.min(x, window.innerWidth - r.width - 6) + 'px';
    m.style.top = Math.min(y, window.innerHeight - r.height - 6) + 'px';
    openMenu = m;
  }

  function menuFromButton(button, items) {
    const r = button.getBoundingClientRect();
    menu(r.left, r.bottom + 4, items);
  }

  // ── Feuille modale ───────────────────────────────────────────────────────

  /** Ouvre une feuille ; `build(body, close)` la remplit. Rend une fonction de fermeture. */
  function sheet(build, opts = {}) {
    const back = document.createElement('div');
    back.className = 'sheet-back';
    const box = document.createElement('div');
    box.className = 'sheet';
    if (opts.width) box.style.width = opts.width + 'px';
    back.appendChild(box);
    document.body.appendChild(back);
    const close = () => { back.remove(); document.removeEventListener('keydown', onKey, true); };
    const onKey = (e) => { if (e.key === 'Escape') { e.stopPropagation(); close(); } };
    document.addEventListener('keydown', onKey, true);
    back.addEventListener('mousedown', (e) => { if (e.target === back) close(); });
    build(box, close);
    const first = box.querySelector('input,select,textarea,button');
    if (first) first.focus();
    return close;
  }

  /** Fragment HTML → premier élément. */
  function h(html) {
    const t = document.createElement('template');
    t.innerHTML = html.trim();
    return t.content.firstElementChild;
  }

  function relativeDate(ms) {
    if (!ms) return '';
    const diff = (Date.now() - ms) / 1000;
    const rtf = new Intl.RelativeTimeFormat('fr', { numeric: 'auto' });
    if (diff < 60) return "à l'instant";
    if (diff < 3600) return rtf.format(-Math.round(diff / 60), 'minute');
    if (diff < 86400) return rtf.format(-Math.round(diff / 3600), 'hour');
    if (diff < 30 * 86400) return rtf.format(-Math.round(diff / 86400), 'day');
    if (diff < 365 * 86400) return rtf.format(-Math.round(diff / (30 * 86400)), 'month');
    return rtf.format(-Math.round(diff / (365 * 86400)), 'year');
  }

  function dateTime(ms) {
    return new Date(ms).toLocaleString('fr-FR', { day: 'numeric', month: 'short', year: 'numeric', hour: '2-digit', minute: '2-digit' });
  }

  /** Dégradé de remplacement déterministe, pour qu'une vignette sans image ait l'air voulue. */
  function placeholder(seed) {
    let hash = 5381;
    for (const ch of String(seed)) hash = ((hash * 33) + ch.charCodeAt(0)) & 0xffffff;
    const hue = hash % 360;
    return `linear-gradient(135deg, hsl(${hue} 30% 36%), hsl(${(hue + 30) % 360} 38% 20%))`;
  }

  window.UI = { esc, bytes, patchList, menu, menuFromButton, closeMenu, sheet, h, relativeDate, dateTime, placeholder };
}());
