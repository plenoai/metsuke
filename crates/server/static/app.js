/* Metsuke — Application shell scripts */

// ---------------------------------------------------------------------------
// SWR (Stale-While-Revalidate) fetch helper
// ---------------------------------------------------------------------------

const _swrCache = {};
async function swrFetch(url, { maxAge = 60000 } = {}) {
  const entry = _swrCache[url];
  const now = Date.now();

  if (entry && now - entry.ts < maxAge) return entry.data;

  try {
    const resp = await fetch(url);
    if (!resp.ok) throw new Error(resp.statusText);
    const data = await resp.json();
    _swrCache[url] = { data, ts: now };
    return data;
  } catch (e) {
    if (entry) return entry.data;
    throw e;
  }
}

// ---------------------------------------------------------------------------
// Shared HTML Sanitizer
// ---------------------------------------------------------------------------

const _sanitizer = { sanitizer: new Sanitizer({
  attributes: ['style', 'class', 'id', 'role', 'tabindex',
    'aria-expanded', 'aria-label', 'aria-live', 'aria-hidden',
    'href', 'target', 'rel', 'scope', 'title', 'disabled', 'value', 'selected',
    'width', 'height', 'viewBox', 'fill', 'd', 'xmlns',
    'placeholder', 'type', 'name', 'for',
  ],
}) };

// ---------------------------------------------------------------------------
// Theme switcher
// ---------------------------------------------------------------------------

(function() {
  var sel = document.getElementById('theme-select');
  if (!sel) return;
  var saved = localStorage.getItem('metsuke-theme') || 'metsuke-dark';
  sel.value = saved;
  sel.addEventListener('change', function() {
    var theme = sel.value;
    document.documentElement.setAttribute('data-theme', theme);
    localStorage.setItem('metsuke-theme', theme);
    // Swap github-markdown-css if needed
    var link = document.getElementById('github-markdown-css');
    if (link) {
      var isLight = theme === 'github-light' || theme === 'github-light-hc';
      var newHref = isLight
        ? 'https://cdn.jsdelivr.net/npm/github-markdown-css@5.9.0/github-markdown-light.min.css'
        : 'https://cdn.jsdelivr.net/npm/github-markdown-css@5.9.0/github-markdown-dark.min.css';
      if (link.href !== newHref) {
        link.removeAttribute('integrity');
        link.href = newHref;
      }
    }
  });
})();

// ---------------------------------------------------------------------------
// Global keyboard shortcuts (when not focused on input/select/textarea)
// ---------------------------------------------------------------------------

document.addEventListener('keydown', function(e) {
  const tag = (document.activeElement && document.activeElement.tagName) || '';
  if (tag === 'INPUT' || tag === 'SELECT' || tag === 'TEXTAREA') return;
  if (e.ctrlKey || e.metaKey || e.altKey) return;

  switch (e.key) {
    case 'g':
      document._metsukeNavPending = true;
      setTimeout(function() { document._metsukeNavPending = false; }, 800);
      return;
    case 'r':
      if (document._metsukeNavPending) { document._metsukeNavPending = false; window.location = '/repos'; e.preventDefault(); }
      return;
    case 'a':
      if (document._metsukeNavPending) { document._metsukeNavPending = false; window.location = '/audit'; e.preventDefault(); }
      return;
    case 's':
      if (document._metsukeNavPending) { document._metsukeNavPending = false; window.location = '/settings'; e.preventDefault(); }
      return;
    case '/':
      e.preventDefault();
      var searchEl = document.getElementById('search-input') || document.getElementById('filter-repo');
      if (searchEl) { searchEl.focus(); searchEl.select(); }
      return;
    case 'j':
    case 'k': {
      var cards = Array.from(document.querySelectorAll('.repo-card:not([hidden]), .pr-item, .audit-table tbody tr'));
      if (cards.length === 0) return;
      e.preventDefault();
      var current = document.querySelector('.repo-card.kbd-focus, .pr-item.kbd-focus, .audit-table tbody tr.kbd-focus');
      var idx = current ? cards.indexOf(current) : -1;
      if (current) current.classList.remove('kbd-focus');
      idx = e.key === 'j' ? Math.min(idx + 1, cards.length - 1) : Math.max(idx - 1, 0);
      cards[idx].classList.add('kbd-focus');
      cards[idx].scrollIntoView({ block: 'nearest', behavior: 'smooth' });
      return;
    }
    case 'o':
    case 'Enter': {
      var focused = document.querySelector('.kbd-focus');
      if (!focused) return;
      var link = focused.querySelector('a[href]');
      if (link) { e.preventDefault(); window.location = link.href; }
      return;
    }
    case '?':
      e.preventDefault();
      toggleShortcutHelp();
      return;
  }
});

// ---------------------------------------------------------------------------
// Keyboard shortcut help overlay
// ---------------------------------------------------------------------------

function toggleShortcutHelp() {
  var overlay = document.getElementById('kbd-help-overlay');
  if (overlay) { overlay.remove(); return; }
  overlay = document.createElement('div');
  overlay.id = 'kbd-help-overlay';
  overlay.setAttribute('role', 'dialog');
  overlay.setAttribute('aria-label', 'キーボードショートカット');
  overlay.setHTML(
    '<div class="kbd-help-backdrop"></div>' +
    '<div class="kbd-help-panel" tabindex="-1" role="document">' +
      '<div class="kbd-help-title">キーボードショートカット</div>' +
      '<table class="kbd-help-table">' +
        '<tr><td class="kbd-key">g r</td><td>リポジトリ一覧</td></tr>' +
        '<tr><td class="kbd-key">g a</td><td>監査ログ</td></tr>' +
        '<tr><td class="kbd-key">g s</td><td>設定</td></tr>' +
        '<tr><td class="kbd-key">/</td><td>検索にフォーカス</td></tr>' +
        '<tr><td class="kbd-key">j / k</td><td>リスト上下移動</td></tr>' +
        '<tr><td class="kbd-key">o</td><td>選択項目を開く</td></tr>' +
        '<tr><td class="kbd-key">?</td><td>このヘルプを表示</td></tr>' +
      '</table>' +
      '<button class="verify-btn kbd-help-close">閉じる (Esc)</button>' +
    '</div>', _sanitizer);
  document.body.appendChild(overlay);
  var _prevFocus = document.activeElement;
  var panel = overlay.querySelector('.kbd-help-panel');
  function closeOverlay() {
    overlay.remove();
    if (_prevFocus) _prevFocus.focus();
  }
  overlay.addEventListener('keydown', function(e) {
    if (e.key === 'Escape') { closeOverlay(); return; }
    if (e.key === 'Tab') {
      var focusable = overlay.querySelectorAll('button, [tabindex="-1"]');
      var first = focusable[0];
      var last = focusable[focusable.length - 1];
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault(); last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault(); first.focus();
      }
    }
  });
  overlay.querySelector('.kbd-help-backdrop').onclick = closeOverlay;
  overlay.querySelector('.kbd-help-close').onclick = closeOverlay;
  panel.focus();
}

// ---------------------------------------------------------------------------
// Print date stamp
// ---------------------------------------------------------------------------

window.addEventListener('beforeprint', function() {
  var h = document.querySelector('.header');
  if (h) h.setAttribute('data-print-date', '出力日時: ' + new Date().toLocaleString('ja-JP'));
});
