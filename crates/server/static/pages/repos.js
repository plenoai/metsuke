const PAGE_SIZE = 50;

let allRepos = [];
let filteredRepos = [];
let currentPage = 1;
let activeOrg = '';
let currentSort = 'activity';

// ---------------------------------------------------------------------------
// Data-driven filter / sort / paginate
// ---------------------------------------------------------------------------

function computeFiltered() {
  const q = (document.getElementById('search-input')?.value || '').toLowerCase();
  let list = allRepos;

  if (activeOrg) {
    list = list.filter(r => r.owner.toLowerCase() === activeOrg.toLowerCase());
  }
  if (q) {
    list = list.filter(r =>
      r.full_name.toLowerCase().includes(q) ||
      (r.description || '').toLowerCase().includes(q) ||
      (r.language || '').toLowerCase().includes(q)
    );
  }

  const mode = currentSort;
  list = [...list].sort((a, b) => {
    if (mode === 'activity') return (b.pushed_at || '').localeCompare(a.pushed_at || '');
    return a.full_name.toLowerCase().localeCompare(b.full_name.toLowerCase());
  });

  filteredRepos = list;
}

function totalPages() {
  return Math.max(1, Math.ceil(filteredRepos.length / PAGE_SIZE));
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

function renderCard(r) {
  return `<div class="repo-card" id="repo-${esc(r.full_name.replace('/', '-'))}">
    <div class="repo-info">
      <div class="repo-name">
        <a href="/repos/${encodeURIComponent(r.owner)}/${encodeURIComponent(r.name)}">${esc(r.full_name)}</a>
        <a class="gh-link" href="https://github.com/${encodeURIComponent(r.owner)}/${encodeURIComponent(r.name)}" target="_blank" rel="noopener" title="GitHub で開く" aria-label="${esc(r.full_name)} を GitHub で開く">
          <svg viewBox="0 0 16 16"><path d="M3.75 2h3.5a.75.75 0 010 1.5H4.56l6.22 6.22a.75.75 0 11-1.06 1.06L3.5 4.56v2.69a.75.75 0 01-1.5 0v-3.5A1.75 1.75 0 013.75 2z"/><path d="M9.25 3.5a.75.75 0 010-1.5h3A1.75 1.75 0 0114 3.75v8.5A1.75 1.75 0 0112.25 14h-8.5A1.75 1.75 0 012 12.25v-3a.75.75 0 011.5 0v3c0 .138.112.25.25.25h8.5a.25.25 0 00.25-.25v-8.5a.25.25 0 00-.25-.25h-3z"/></svg>
        </a>
      </div>
      <div class="repo-meta">
        ${r.private ? '<span class="badge badge-private">private</span>' : ''}
        ${r.language ? `<span>${esc(r.language)}</span>` : ''}
        ${r.default_branch ? `<span>${esc(r.default_branch)}</span>` : ''}
        ${r.pushed_at ? `<span>${timeAgo(r.pushed_at)}</span>` : ''}
      </div>
    </div>
  </div>`;
}

function renderPagination() {
  const tp = totalPages();
  if (tp <= 1) return '';
  const start = Math.max(1, currentPage - 2);
  const end = Math.min(tp, currentPage + 2);
  let html = '<div class="pagination">';
  if (currentPage > 1) html += `<button class="verify-btn pagination-btn" data-action="page" data-page="${currentPage - 1}">‹</button>`;
  for (let i = start; i <= end; i++) {
    if (i === currentPage) {
      html += `<span class="pagination-current">${i}</span>`;
    } else {
      html += `<button class="verify-btn pagination-btn" data-action="page" data-page="${i}">${i}</button>`;
    }
  }
  if (currentPage < tp) html += `<button class="verify-btn pagination-btn" data-action="page" data-page="${currentPage + 1}">›</button>`;
  html += `<span class="pagination-counter">${PAGE_SIZE * (currentPage - 1) + 1}–${Math.min(PAGE_SIZE * currentPage, filteredRepos.length)} / ${filteredRepos.length}</span>`;
  html += '</div>';
  return html;
}

function renderPage() {
  computeFiltered();
  if (currentPage > totalPages()) currentPage = totalPages();

  const container = document.getElementById('repo-list');
  const total = allRepos.length;
  const matched = filteredRepos.length;
  const titleText = matched < total ? `リポジトリ (${matched} / ${total})` : `リポジトリ (${total})`;
  document.getElementById('repos-title').textContent = titleText;
  document.title = `Repos (${matched}) — Metsuke`;

  if (total === 0) {
    container.setHTML(`<div class="empty-state">
      <div class="empty-state-cta">リポジトリが見つかりません</div>
      <a class="btn btn--link" href="/settings">GitHub Appをインストール</a>
    </div>`, _sanitizer);
    return;
  }

  const prevQ = document.getElementById('search-input')?.value || '';
  const start = (currentPage - 1) * PAGE_SIZE;
  const pageRepos = filteredRepos.slice(start, start + PAGE_SIZE);

  const searchBar = `<div class="search-bar-wrap">
    <input type="search" id="search-input" class="policy-select repo-search" placeholder="リポジトリを検索…" aria-label="リポジトリを検索" value="${esc(prevQ)}">
  </div>`;

  container.setHTML(searchBar
    + renderPagination()
    + '<div class="repo-grid">' + pageRepos.map(renderCard).join('') + '</div>'
    + renderPagination(), _sanitizer);

  const orgs = [...new Set(allRepos.map(r => r.owner))].sort();
  const orgFilter = document.getElementById('org-filter');
  if (orgs.length > 1) {
    orgFilter.setHTML('<option value="">全組織</option>' + orgs.map(o => `<option value="${esc(o)}" ${activeOrg === o ? 'selected' : ''}>${esc(o)}</option>`).join(''), _sanitizer);
    orgFilter.hidden = false;
  }
}

// ---------------------------------------------------------------------------
// Event handlers
// ---------------------------------------------------------------------------

let searchTimer = null;
function onSearchInput() {
  clearTimeout(searchTimer);
  searchTimer = setTimeout(() => { currentPage = 1; renderPage(); }, 150);
}

function goToPage(p) {
  currentPage = p;
  renderPage();
  window.scrollTo({ top: 0, behavior: 'smooth' });
}

function sortRepos(mode) {
  currentSort = mode;
  currentPage = 1;
  renderPage();
}


function filterByOrg(org) {
  activeOrg = org;
  currentPage = 1;
  renderPage();
}

// ---------------------------------------------------------------------------
// Data loading: fetch JSON + SSE job events
// ---------------------------------------------------------------------------

async function fetchRepos() {
  const resp = await fetchWithTimeout('/api/repos');
  if (!resp.ok) throw new Error(await resp.text());
  return resp.json();
}

async function loadRepos() {
  try {
    allRepos = await fetchRepos();
    renderPage();
  } catch (e) {
    renderLoadError('repo-list', classifyError(e), 'loadRepos');
    return;
  }

  // Subscribe to job events for live updates
  const es = new EventSource('/api/events');
  es.addEventListener('job', async (e) => {
    const event = JSON.parse(e.data);
    if (event.type === 'repos_synced') {
      try {
        allRepos = await fetchRepos();
        renderPage();
      } catch (_) { /* will retry on next event */ }
    }
  });
  window.addEventListener('pagehide', () => es.close());
}

// Event delegation
document.getElementById('org-filter').addEventListener('change', function() { filterByOrg(this.value); });
document.getElementById('sort-select').addEventListener('change', function() { sortRepos(this.value); });
document.getElementById('repo-list').addEventListener('click', function(e) {
  const pageBtn = e.target.closest('[data-action="page"]');
  if (pageBtn) goToPage(Number(pageBtn.dataset.page));
});
document.getElementById('repo-list').addEventListener('input', function(e) {
  if (e.target.id === 'search-input') onSearchInput();
});

loadRepos();
