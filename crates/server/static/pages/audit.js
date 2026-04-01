let currentOffset = 0;
const PAGE_SIZE = 50;
let allEntries = [];


function typeBadge(type) {
  if (type === 'release') return '<span class="type-badge type-release">release</span>';
  if (type === 'pr') return '<span class="type-badge type-pr">pr</span>';
  return '<span class="type-badge type-repo">repo</span>';
}

function resultBadges(e) {
  let b = '';
  if (e.pass > 0) b += `<span class="badge badge-pass" title="PASS">PASS ${e.pass}</span>`;
  if (e.review > 0) b += `<span class="badge badge-review" title="REVIEW">REV ${e.review}</span>`;
  if (e.fail > 0) b += `<span class="badge badge-fail" title="FAIL">FAIL ${e.fail}</span>`;
  return b || '<span class="text-muted-dash">—</span>';
}

async function loadAudit() {
  const type = document.getElementById('filter-type').value;
  const container = document.getElementById('audit-content');
  container.setHTML(renderSkeleton(4), _sanitizer);

  try {
    let url = `/api/audit-history?limit=${PAGE_SIZE}&offset=${currentOffset}`;
    if (type) url += `&type=${encodeURIComponent(type)}`;
    const fromDate = document.getElementById('filter-from').value;
    const toDate = document.getElementById('filter-to').value;
    if (fromDate) url += `&from_date=${encodeURIComponent(fromDate)}`;
    if (toDate) url += `&to_date=${encodeURIComponent(toDate)}`;
    const repoFilter = document.getElementById('filter-repo').value.trim();
    if (repoFilter) {
      const parts = repoFilter.split('/');
      if (parts.length === 2) {
        url += `&owner=${encodeURIComponent(parts[0])}&repo=${encodeURIComponent(parts[1])}`;
      } else {
        url += `&owner=${encodeURIComponent(repoFilter)}`;
      }
    }
    const resp = await fetchWithTimeout(url);
    const entries = resp.ok ? await resp.json() : [];
    allEntries = entries;

    if (entries.length === 0 && currentOffset === 0) {
      container.setHTML(`<div class="empty-state">
        <div class="empty-state-cta">監査ログはまだありません</div>
        <a class="btn btn--link" href="/repos">リポジトリ一覧へ</a>
      </div>`, _sanitizer);
      document.getElementById('audit-pagination').setHTML('', _sanitizer);
      return;
    }

    const rows = entries.map(e => `<tr>
      <td class="audit-timestamp">${new Date(e.verified_at + 'Z').toLocaleString('ja-JP')}</td>
      <td>${typeBadge(e.type)}</td>
      <td><a href="/repos/${encodeURIComponent(e.owner)}/${encodeURIComponent(e.repo)}" class="audit-repo-link">${esc(e.owner)}/${esc(e.repo)}</a></td>
      <td class="audit-ref">${esc(e.target_ref)}</td>
      <td class="audit-policy">${esc(e.policy)}</td>
      <td class="audit-results">${resultBadges(e)}</td>
    </tr>`).join('');

    container.setHTML(`<div class="card findings-card">
      <table class="audit-table">
        <thead>
          <tr><th scope="col">日時</th><th scope="col">タイプ</th><th scope="col">リポジトリ</th><th scope="col">対象</th><th scope="col">ポリシー</th><th scope="col">結果</th></tr>
        </thead>
        <tbody>${rows}</tbody>
      </table>
    </div>`, _sanitizer);

    // Update page title with context
    const filterParts = [];
    if (type) filterParts.push(type);
    if (repoFilter) filterParts.push(repoFilter);
    document.title = filterParts.length > 0
      ? `監査ログ: ${filterParts.join(' ')} (${entries.length}件) — Metsuke`
      : `監査ログ (${currentOffset + 1}-${currentOffset + entries.length}) — Metsuke`;

    // Pagination
    const pag = document.getElementById('audit-pagination');
    pag.setHTML(`
      <button data-action="prev-page" ${currentOffset === 0 ? 'disabled' : ''}>前へ</button>
      <span class="pagination-info">${currentOffset + 1}〜${currentOffset + entries.length} 件</span>
      <button data-action="next-page" ${entries.length < PAGE_SIZE ? 'disabled' : ''}>次へ</button>
    `, _sanitizer);
  } catch (e) {
    renderLoadError('audit-content', '監査ログの取得に失敗しました。', 'loadAudit');
  }
}

function prevPage() {
  currentOffset = Math.max(0, currentOffset - PAGE_SIZE);
  loadAudit();
}

function nextPage() {
  currentOffset += PAGE_SIZE;
  loadAudit();
}

function exportAuditCSV() {
  const btn = document.getElementById('export-csv-btn');
  btn.textContent = '出力中…';
  btn.disabled = true;
  let url = '/api/audit-history/export?';
  const type = document.getElementById('filter-type').value;
  if (type) url += `type=${encodeURIComponent(type)}&`;
  const fromDate = document.getElementById('filter-from').value;
  const toDate = document.getElementById('filter-to').value;
  if (fromDate) url += `from_date=${encodeURIComponent(fromDate)}&`;
  if (toDate) url += `to_date=${encodeURIComponent(toDate)}&`;
  const repoFilter = document.getElementById('filter-repo').value.trim();
  if (repoFilter) {
    const parts = repoFilter.split('/');
    if (parts.length === 2) {
      url += `owner=${encodeURIComponent(parts[0])}&repo=${encodeURIComponent(parts[1])}&`;
    } else {
      url += `owner=${encodeURIComponent(repoFilter)}&`;
    }
  }
  window.location = url;
  setTimeout(() => { btn.textContent = 'CSV出力'; btn.disabled = false; }, 1500);
}

function resetAndLoad() {
  currentOffset = 0;
  loadAudit();
}

function applyPreset(val) {
  const from = document.getElementById('filter-from');
  const to = document.getElementById('filter-to');
  const now = new Date();
  const y = now.getFullYear();
  const m = now.getMonth();
  const q = Math.floor(m / 3);

  if (val === 'this-quarter') {
    from.value = `${y}-${String(q * 3 + 1).padStart(2, '0')}-01`;
    to.value = '';
  } else if (val === 'last-quarter') {
    const lq = q === 0 ? 3 : q - 1;
    const ly = q === 0 ? y - 1 : y;
    from.value = `${ly}-${String(lq * 3 + 1).padStart(2, '0')}-01`;
    to.value = `${q === 0 ? y - 1 : y}-${String(q * 3 + 1).padStart(2, '0')}-01`;
  } else if (val === 'ytd') {
    from.value = `${y}-01-01`;
    to.value = '';
  } else if (val === 'last-12m') {
    const past = new Date(now);
    past.setFullYear(past.getFullYear() - 1);
    from.value = past.toISOString().slice(0, 10);
    to.value = '';
  } else {
    from.value = '';
    to.value = '';
  }
  resetAndLoad();
}

let debounceTimer = null;
function debounceLoad() {
  clearTimeout(debounceTimer);
  debounceTimer = setTimeout(() => resetAndLoad(), 400);
}

// Event listeners
document.getElementById('filter-type').addEventListener('change', resetAndLoad);
document.getElementById('filter-repo').addEventListener('change', resetAndLoad);
document.getElementById('filter-repo').addEventListener('input', debounceLoad);
document.getElementById('filter-from').addEventListener('change', resetAndLoad);
document.getElementById('filter-to').addEventListener('change', resetAndLoad);
document.getElementById('filter-preset').addEventListener('change', function() { applyPreset(this.value); });
document.getElementById('export-csv-btn').addEventListener('click', exportAuditCSV);
document.getElementById('audit-pagination').addEventListener('click', function(e) {
  const prev = e.target.closest('[data-action="prev-page"]');
  if (prev && !prev.disabled) prevPage();
  const next = e.target.closest('[data-action="next-page"]');
  if (next && !next.disabled) nextPage();
});

loadAudit();
