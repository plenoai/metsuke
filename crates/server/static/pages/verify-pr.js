const OWNER = document.currentScript.dataset.owner;
const REPO = document.currentScript.dataset.repo;
let allPRs = [];
let selectedPR = null;

function prState(pr) {
  if (pr.merged_at) return 'merged';
  if (pr.state === 'closed') return 'closed';
  return 'open';
}

function prStateBadge(pr) {
  const s = prState(pr);
  if (s === 'merged') return '<span class="pr-state pr-state--merged">merged</span>';
  if (s === 'closed') return '<span class="pr-state pr-state--closed">closed</span>';
  return '<span class="pr-state pr-state--open">open</span>';
}

function togglePR(num) {
  if (selectedPR === num) {
    selectedPR = null;
  } else {
    selectedPR = num;
  }
  renderCurrentList();
  if (selectedPR !== null) loadCachedFindings(selectedPR);
}

async function loadCachedFindings(num) {
  try {
    const resp = await fetchWithTimeout(`/api/repos/${OWNER}/${REPO}/verify-pr/${num}/latest`);
    if (!resp.ok) return;
    const data = await resp.json();
    if (data.findings) {
      openFindingsSidebar(`PR #${num} 検証結果`, data.findings, {
        owner: OWNER, repo: REPO, target_ref: `#${num}`, policy: data.profile_name || 'default',
      });
      const btn = document.getElementById(`pr-verify-btn-${num}`);
      if (btn) btn.textContent = '再検証';
    }
  } catch (_) {}
}

function renderPRList(prs) {
  const container = document.getElementById('pr-list');
  if (prs.length === 0) {
    container.setHTML('<div class="empty-state">PRはありません</div>', _sanitizer);
    return;
  }
  container.setHTML('<div class="pr-list">' + prs.map(pr => {
    const isSelected = selectedPR === pr.pr_number;
    return `
    <div class="pr-item${isSelected ? ' is-selected' : ''}" data-action="toggle-pr" data-pr="${pr.pr_number}">
      <div class="pr-item__info">
        <div class="pr-item__title">#${pr.pr_number} ${esc(pr.title)}</div>
        <div class="pr-item__meta">
          ${prStateBadge(pr)}
          <span>${esc(pr.author)}</span>
          <span>${new Date(pr.updated_at).toLocaleDateString('ja-JP')}</span>
          ${pr.draft ? '<span class="pr-state--draft">draft</span>' : ''}
        </div>
      </div>
      <div class="inline-row">
        <div id="pr-result-${pr.pr_number}" class="inline-row--tight"></div>
      </div>
    </div>
    ${isSelected ? `<div class="pr-detail" id="pr-detail-${pr.pr_number}">
      <div class="pr-detail__header">
        <a href="https://github.com/${OWNER}/${REPO}/pull/${pr.pr_number}" target="_blank" rel="noopener" class="pr-detail__link">GitHub で開く ↗</a>
        <button class="btn--verify" id="pr-verify-btn-${pr.pr_number}" data-action="verify-pr" data-pr="${pr.pr_number}">検証</button>
      </div>
    </div>` : ''}`;
  }).join('') + '</div>', _sanitizer);
}

let _currentFiltered = [];
function renderCurrentList() {
  renderPRList(_currentFiltered);
  applyAuditBadges();
}

function applyFilter() {
  const filter = document.getElementById('pr-filter').value;
  let filtered = allPRs;
  if (filter) filtered = allPRs.filter(pr => prState(pr) === filter);
  filtered.sort((a, b) => new Date(b.updated_at) - new Date(a.updated_at));
  _currentFiltered = filtered;
  renderPRList(filtered);
}

// 過去の検証結果をPRバッジに反映
let _auditCache = {};
async function loadAuditResults() {
  try {
    const entries = await swrFetch(`/api/audit-history?type=pr&owner=${OWNER}&repo=${REPO}&limit=100`);
    _auditCache = {};
    for (const e of entries) {
      const num = e.target_ref.replace('#', '');
      if (!_auditCache[num]) _auditCache[num] = e;
    }
  } catch (_) {}
}

function applyAuditBadges() {
  for (const [num, e] of Object.entries(_auditCache)) {
    const el = document.getElementById(`pr-result-${num}`);
    if (el && !el.textContent.trim()) {
      el.setHTML(compactBadges(e.pass, e.fail, e.review), _sanitizer);
    }
  }
}

async function loadPRs() {
  try {
    allPRs = await swrFetch(`/api/repos/${OWNER}/${REPO}/pulls`);
    applyFilter();
    await loadAuditResults();
    applyAuditBadges();
    autoVerifyIfFew();
  } catch (e) {
    renderLoadError('pr-list', classifyError(e), 'loadPRs');
  }
}

// 検証結果が無いPRが表示中10件未満なら自動検証
async function autoVerifyIfFew() {
  const unverified = _currentFiltered.filter(pr => !_auditCache[pr.pr_number]);
  if (unverified.length === 0 || _currentFiltered.length >= 10) return;
  const policy = document.getElementById('pr-policy').value;
  for (const pr of unverified) {
    try {
      const resp = await fetchWithTimeout(`/api/repos/${OWNER}/${REPO}/verify-pr/${pr.pr_number}?policy=${encodeURIComponent(policy)}`, { method: 'POST' }, 60000);
      if (!resp.ok) continue;
      const data = await resp.json();
      const c = countFindings(data.findings);
      _auditCache[pr.pr_number] = { pass: c.pass, fail: c.fail, review: c.review };
      const el = document.getElementById(`pr-result-${pr.pr_number}`);
      if (el) el.setHTML(compactBadges(c.pass, c.fail, c.review), _sanitizer);
    } catch (_) {}
  }
}

async function verifyPRById(num) {
  const policy = document.getElementById('pr-policy').value;
  const btn = document.getElementById(`pr-verify-btn-${num}`);
  const resultEl = document.getElementById(`pr-result-${num}`);
  if (!btn) return;

  btn.disabled = true;
  btn.textContent = '検証中…';
  btn.classList.add('is-running');
  openSidebar(`PR #${num} 検証中…`, '<div class="loading" role="status">検証を実行中</div>', '');

  try {
    const resp = await fetchWithTimeout(`/api/repos/${OWNER}/${REPO}/verify-pr/${num}?policy=${encodeURIComponent(policy)}`, { method: 'POST' }, 60000);
    if (!resp.ok) throw new Error(await resp.text());
    const data = await resp.json();
    const c = countFindings(data.findings);
    if (resultEl) resultEl.setHTML(compactBadges(c.pass, c.fail, c.review), _sanitizer);
    openFindingsSidebar(`PR #${num} 検証結果`, data.findings, {
      owner: OWNER, repo: REPO, target_ref: `#${num}`, policy: data.profile_name || policy,
    });
    btn.textContent = '再検証';
  } catch (e) {
    if (resultEl) resultEl.setHTML('<span class="badge badge--fail" title="ERROR">ERR</span>', _sanitizer);
    openSidebar('エラー', renderErrorCard(classifyError(e)), '');
    btn.textContent = '再試行';
  }
  btn.disabled = false;
  btn.classList.remove('is-running');
}

// Event listeners
document.getElementById('pr-filter').addEventListener('change', applyFilter);
document.getElementById('pr-list').addEventListener('click', function(e) {
  const verifyBtn = e.target.closest('[data-action="verify-pr"]');
  if (verifyBtn) {
    e.stopPropagation();
    verifyPRById(Number(verifyBtn.dataset.pr));
    return;
  }
  const prItem = e.target.closest('[data-action="toggle-pr"]');
  if (prItem) {
    togglePR(Number(prItem.dataset.pr));
  }
});

loadPRs();

// SSE: バックグラウンド同期完了時にPR一覧を再取得
const es = new EventSource('/api/events');
es.addEventListener('job', async (e) => {
  try {
    const ev = JSON.parse(e.data);
    if (ev.type === 'pulls_synced' && ev.owner === OWNER && ev.repo === REPO) {
      delete _swrCache[`/api/repos/${OWNER}/${REPO}/pulls`];
      loadPRs();
    }
  } catch (_) {}
});
window.addEventListener('pagehide', () => es.close());
