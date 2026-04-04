const OWNER = document.currentScript.dataset.owner;
const REPO = document.currentScript.dataset.repo;
let allReleases = [];
let selectedIdx = null;

function renderReleaseList(releases) {
  allReleases = releases;

  const baseSelect = document.getElementById('base-tag');
  const headSelect = document.getElementById('head-tag');
  if (releases.length < 2) {
    baseSelect.setHTML('<option value="">タグが不足しています</option>', _sanitizer);
    headSelect.setHTML('<option value="">タグが不足しています</option>', _sanitizer);
  } else {
    const opts = releases.map(r => `<option value="${esc(r.tag_name)}">${esc(r.tag_name)}</option>`).join('');
    baseSelect.setHTML(opts, _sanitizer);
    headSelect.setHTML(opts, _sanitizer);
    baseSelect.value = releases[1].tag_name;
    headSelect.value = releases[0].tag_name;
  }

  renderCurrentList();
}

function renderCurrentList() {
  const container = document.getElementById('release-list');
  const releases = allReleases;
  if (releases.length === 0) {
    container.setHTML('<div class="empty-state">リリースはありません</div>', _sanitizer);
    return;
  }
  container.setHTML('<div class="release-list">' + releases.map((rel, i) => {
    const prevTag = i < releases.length - 1 ? releases[i + 1].tag_name : null;
    const date = rel.published_at || rel.created_at;
    const name = rel.name || rel.tag_name;
    const isSelected = selectedIdx === i;
    return `<div class="release-item${isSelected ? ' is-selected' : ''}" data-action="toggle-release" data-idx="${i}">
      <div class="release-item__info">
        <div class="release-item__tag">
          ${esc(rel.tag_name)}
          ${name !== rel.tag_name ? `<span class="release-item__tag-sub">${esc(name)}</span>` : ''}
        </div>
        <div class="release-item__meta">
          <span>${esc(rel.author)}</span>
          <span>${new Date(date).toLocaleDateString('ja-JP')}</span>
          ${rel.prerelease ? '<span class="release-item--prerelease">pre-release</span>' : ''}
          ${rel.draft ? '<span class="release-item--prerelease">draft</span>' : ''}
        </div>
      </div>
      <div class="inline-row">
        <div id="release-result-${i}" class="inline-row--tight"></div>
      </div>
    </div>
    ${isSelected ? renderReleaseDetail(rel, i, prevTag) : ''}`;
  }).join('') + '</div>', _sanitizer);

  applyReleaseAuditBadges();
}

function renderReleaseDetail(rel, idx, prevTag) {
  const bodySnippet = rel.body ? `<div class="release-detail__body">${esc(rel.body)}</div>` : '';
  return `<div class="release-detail" id="release-detail-${idx}">
    <div class="release-detail__header">
      <a href="${esc(rel.html_url || `https://github.com/${OWNER}/${REPO}/releases/tag/${encodeURIComponent(rel.tag_name)}`)}" target="_blank" rel="noopener" class="release-detail__link">GitHub で開く ↗</a>
      ${prevTag
        ? `<button class="btn--verify" id="release-verify-btn-${idx}" data-action="verify-release" data-base="${esc(prevTag)}" data-head="${esc(rel.tag_name)}" data-idx="${idx}">検証</button>`
        : `<span class="release-item__initial">初回リリース</span>`}
    </div>
    ${bodySnippet}
  </div>`;
}

function toggleRelease(idx) {
  if (selectedIdx === idx) {
    selectedIdx = null;
  } else {
    selectedIdx = idx;
  }
  renderCurrentList();
  if (selectedIdx !== null) loadCachedReleaseFindings(selectedIdx);
}

async function loadCachedReleaseFindings(idx) {
  const rel = allReleases[idx];
  const prevTag = idx < allReleases.length - 1 ? allReleases[idx + 1].tag_name : null;
  if (!prevTag) return;
  const ref = `${prevTag}..${rel.tag_name}`;

  const cached = _releaseAuditCache[ref];
  if (cached && cached.findings) {
    openFindingsSidebar(`${prevTag} .. ${rel.tag_name} 検証結果`, cached.findings, {
      owner: OWNER, repo: REPO, target_ref: ref, policy: cached.policy || 'default',
    });
    const btn = document.getElementById(`release-verify-btn-${idx}`);
    if (btn) btn.textContent = '再検証';
    return;
  }

  try {
    const resp = await fetchWithTimeout(`/api/repos/${OWNER}/${REPO}/verify-release/latest/${encodeURIComponent(ref)}`);
    if (!resp.ok) return;
    const data = await resp.json();
    if (data.findings) {
      openFindingsSidebar(`${prevTag} .. ${rel.tag_name} 検証結果`, data.findings, {
        owner: OWNER, repo: REPO, target_ref: ref, policy: data.profile_name || 'default',
      });
      const btn = document.getElementById(`release-verify-btn-${idx}`);
      if (btn) btn.textContent = '再検証';
    }
  } catch (_) {}
}

let _releaseAuditCache = {};
async function loadReleaseAuditResults() {
  try {
    const entries = await swrFetch(`/api/repos/${OWNER}/${REPO}/verify-release/latest`);
    _releaseAuditCache = {};
    for (const e of entries) {
      _releaseAuditCache[e.target_ref] = e;
    }
  } catch (_) {}
}

function applyReleaseAuditBadges() {
  for (let i = 0; i < allReleases.length; i++) {
    const rel = allReleases[i];
    const prevTag = i < allReleases.length - 1 ? allReleases[i + 1].tag_name : null;
    if (!prevTag) continue;
    const ref = `${prevTag}..${rel.tag_name}`;
    const cached = _releaseAuditCache[ref];
    if (!cached) continue;
    const el = document.getElementById(`release-result-${i}`);
    if (el && !el.textContent.trim()) {
      el.setHTML(compactBadges(cached.pass, cached.fail, cached.review), _sanitizer);
    }
  }
}

async function loadReleases() {
  try {
    const releases = await swrFetch(`/api/repos/${OWNER}/${REPO}/releases`);
    renderReleaseList(releases);
    await loadReleaseAuditResults();
    applyReleaseAuditBadges();
  } catch (e) {
    renderLoadError('release-list', classifyError(e), 'loadReleases');
  }
}

async function verifyRelease() {
  const baseTag = document.getElementById('base-tag').value;
  const headTag = document.getElementById('head-tag').value;
  if (!baseTag || !headTag) return;
  if (baseTag === headTag) { openSidebar('エラー', '<div class="card validation-error">Base TagとHead Tagは異なる値を選択してください</div>', ''); return; }

  const policy = document.getElementById('release-policy').value;
  const btn = document.getElementById('verify-btn');

  btn.disabled = true;
  btn.textContent = '検証中…';
  btn.classList.add('is-running');
  openSidebar('検証中…', '<div class="loading" role="status">検証を実行中</div>', '');

  try {
    const resp = await fetchWithTimeout(`/api/repos/${OWNER}/${REPO}/verify-release?base_tag=${encodeURIComponent(baseTag)}&head_tag=${encodeURIComponent(headTag)}&policy=${encodeURIComponent(policy)}`, { method: 'POST' }, 60000);
    if (!resp.ok) throw new Error(await resp.text());
    const data = await resp.json();
    openFindingsSidebar(`${baseTag} .. ${headTag} 検証結果`, data.findings, {
      owner: OWNER, repo: REPO, target_ref: `${baseTag}..${headTag}`, policy: data.profile_name || policy,
    });
  } catch (e) {
    openSidebar('エラー', renderErrorCard(classifyError(e)), '');
  }
  btn.disabled = false;
  btn.textContent = '検証を実行';
  btn.classList.remove('is-running');
}

async function verifyReleaseByTag(baseTag, headTag, idx, btn) {
  const policy = document.getElementById('release-policy').value;
  btn.disabled = true;
  btn.textContent = '検証中…';
  btn.classList.add('is-running');
  const resultEl = document.getElementById(`release-result-${idx}`);
  openSidebar(`${baseTag} .. ${headTag} 検証中…`, '<div class="loading" role="status">検証を実行中</div>', '');

  try {
    const resp = await fetchWithTimeout(`/api/repos/${OWNER}/${REPO}/verify-release?base_tag=${encodeURIComponent(baseTag)}&head_tag=${encodeURIComponent(headTag)}&policy=${encodeURIComponent(policy)}`, { method: 'POST' }, 60000);
    if (!resp.ok) throw new Error(await resp.text());
    const data = await resp.json();
    const c = countFindings(data.findings);
    if (resultEl) resultEl.setHTML(compactBadges(c.pass, c.fail, c.review), _sanitizer);
    openFindingsSidebar(`${baseTag} .. ${headTag} 検証結果`, data.findings, {
      owner: OWNER, repo: REPO, target_ref: `${baseTag}..${headTag}`, policy: data.profile_name || policy,
    });
    btn.textContent = '再検証';
  } catch (e) {
    if (resultEl) resultEl.setHTML('<span class="badge badge--fail" title="ERROR">ERR</span>', _sanitizer);
    openSidebar('エ��ー', renderErrorCard(classifyError(e)), '');
    btn.textContent = '再試行';
  }
  btn.disabled = false;
  btn.classList.remove('is-running');
}

// Event delegation
document.getElementById('verify-btn').addEventListener('click', verifyRelease);
document.getElementById('release-list').addEventListener('click', function(e) {
  const verifyBtn = e.target.closest('[data-action="verify-release"]');
  if (verifyBtn) {
    e.stopPropagation();
    verifyReleaseByTag(verifyBtn.dataset.base, verifyBtn.dataset.head, Number(verifyBtn.dataset.idx), verifyBtn);
    return;
  }
  const releaseItem = e.target.closest('[data-action="toggle-release"]');
  if (releaseItem) {
    toggleRelease(Number(releaseItem.dataset.idx));
  }
});

loadReleases();

// SSE: バックグラウンド同期完了時にリリース一覧を再取得
const es = new EventSource('/api/events');
es.addEventListener('job', async (e) => {
  try {
    const ev = JSON.parse(e.data);
    if (ev.type === 'releases_synced' && ev.owner === OWNER && ev.repo === REPO) {
      delete _swrCache[`/api/repos/${OWNER}/${REPO}/releases`];
      loadReleases();
    }
  } catch (_) {}
});
window.addEventListener('pagehide', () => es.close());
