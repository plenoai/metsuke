const OWNER = document.currentScript.dataset.owner;
const REPO = document.currentScript.dataset.repo;
let allReleases = [];

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

  const container = document.getElementById('release-list');
  if (releases.length === 0) {
    container.setHTML('<div class="empty-state">リリースはありません</div>', _sanitizer);
    return;
  }
  container.setHTML('<div class="release-list">' + releases.map((rel, i) => {
    const prevTag = i < releases.length - 1 ? releases[i + 1].tag_name : null;
    const date = rel.published_at || rel.created_at;
    const name = rel.name || rel.tag_name;
    return `<div class="release-item">
      <div class="release-item-info">
        <div class="release-item-tag">
          ${esc(rel.tag_name)}
          ${name !== rel.tag_name ? `<span class="release-tag-secondary">${esc(name)}</span>` : ''}
        </div>
        <div class="release-item-meta">
          <span>${esc(rel.author)}</span>
          <span>${new Date(date).toLocaleDateString('ja-JP')}</span>
          ${rel.prerelease ? '<span class="release-prerelease">pre-release</span>' : ''}
          ${rel.draft ? '<span class="release-prerelease">draft</span>' : ''}
        </div>
      </div>
      <div class="inline-row">
        <div id="release-result-${i}" class="inline-row--tight"></div>
        ${prevTag ? `<button class="verify-btn" data-action="verify-release" data-base="${esc(prevTag)}" data-head="${esc(rel.tag_name)}" data-idx="${i}">検証</button>` : `<span class="release-initial">初回リリース</span>`}
      </div>
    </div>`;
  }).join('') + '</div>', _sanitizer);
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
    renderLoadError('release-list', 'リリースの取得に失敗しました。', 'loadReleases');
  }
}

async function verifyRelease() {
  const baseTag = document.getElementById('base-tag').value;
  const headTag = document.getElementById('head-tag').value;
  if (!baseTag || !headTag) return;
  if (baseTag === headTag) { document.getElementById('result-area').setHTML('<div class="card validation-error">Base TagとHead Tagは異なる値を選択してください</div>', _sanitizer); return; }

  const policy = document.getElementById('release-policy').value;
  const btn = document.getElementById('verify-btn');
  const area = document.getElementById('result-area');

  btn.disabled = true;
  btn.textContent = '検証中…';
  btn.classList.add('running');
  area.setHTML('<div class="loading" role="status">検証を実行中</div>', _sanitizer);

  try {
    const resp = await fetchWithTimeout(`/api/repos/${OWNER}/${REPO}/verify-release?base_tag=${encodeURIComponent(baseTag)}&head_tag=${encodeURIComponent(headTag)}&policy=${encodeURIComponent(policy)}`, { method: 'POST' }, 60000);
    if (!resp.ok) throw new Error(await resp.text());
    const data = await resp.json();
    area.setHTML(renderFindingsTable(data.findings, `${esc(baseTag)} .. ${esc(headTag)} 検証結果`), _sanitizer);
  } catch (e) {
    area.setHTML(renderErrorCard(e.message), _sanitizer);
  }
  btn.disabled = false;
  btn.textContent = '検証を実行';
  btn.classList.remove('running');
}

async function verifyReleaseByTag(baseTag, headTag, idx, btn) {
  const policy = document.getElementById('release-policy').value;
  btn.disabled = true;
  btn.textContent = '検証中…';
  btn.classList.add('running');
  const resultEl = document.getElementById(`release-result-${idx}`);

  try {
    const resp = await fetchWithTimeout(`/api/repos/${OWNER}/${REPO}/verify-release?base_tag=${encodeURIComponent(baseTag)}&head_tag=${encodeURIComponent(headTag)}&policy=${encodeURIComponent(policy)}`, { method: 'POST' }, 60000);
    if (!resp.ok) throw new Error(await resp.text());
    const data = await resp.json();
    const c = countFindings(data.findings);
    resultEl.setHTML(compactBadges(c.pass, c.fail, c.review), _sanitizer);
    document.getElementById('result-area').setHTML(renderFindingsTable(data.findings, `${esc(baseTag)} .. ${esc(headTag)} 検証結果`), _sanitizer);
    btn.textContent = '再検証';
  } catch (e) {
    resultEl.setHTML('<span class="badge badge-fail" title="ERROR">ERR</span>', _sanitizer);
    btn.textContent = '再試行';
  }
  btn.disabled = false;
  btn.classList.remove('running');
}

// Event delegation
document.getElementById('verify-btn').addEventListener('click', verifyRelease);
document.getElementById('release-list').addEventListener('click', function(e) {
  const btn = e.target.closest('[data-action="verify-release"]');
  if (btn) {
    verifyReleaseByTag(btn.dataset.base, btn.dataset.head, Number(btn.dataset.idx), btn);
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
