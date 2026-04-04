const OWNER = document.currentScript.dataset.owner;
const REPO = document.currentScript.dataset.repo;

let _lastRepoFindings = null;
let _lastRepoProfile = 'default';

// ---- Compliance visualization ----

const CONTROL_CATEGORIES = [
  { prefix: 'source-', label: 'Source' },
  { prefix: 'build-', label: 'Build' },
  { prefix: 'dep-', label: 'Dependencies' },
  { prefix: 'branch-protection', label: 'Branch Protection' },
  { prefix: 'code-review', label: 'Code Review' },
  { prefix: 'codeowners', label: 'Code Review' },
  { prefix: 'ci-', label: 'CI' },
  { prefix: 'actions-', label: 'CI' },
  { prefix: 'commit-sign', label: 'Signing' },
  { prefix: 'security-', label: 'Security' },
  { prefix: 'secret-scanning', label: 'Security' },
  { prefix: 'vulnerability-', label: 'Security' },
  { prefix: 'dismiss-stale', label: 'Branch Protection' },
  { prefix: 'release-', label: 'Release' },
];

function categorizeFindings(findings) {
  const categories = new Map();
  for (const f of (findings || [])) {
    let catLabel = 'Other';
    for (const cat of CONTROL_CATEGORIES) {
      if (f.control_id.startsWith(cat.prefix)) { catLabel = cat.label; break; }
    }
    if (!categories.has(catLabel)) categories.set(catLabel, []);
    categories.get(catLabel).push(f);
  }
  return categories;
}

function statusToDotClass(status) {
  if (status === 'satisfied') return 'control-grid__dot--pass';
  if (status === 'violated') return 'control-grid__dot--fail';
  if (status === 'indeterminate') return 'control-grid__dot--review';
  return 'control-grid__dot--na';
}

function renderComplianceViz(findings) {
  const c = countFindings(findings);
  const total = c.pass + c.fail + c.review + c.na;
  if (total === 0) return '';

  const pctPass = (c.pass / total * 100).toFixed(1);
  const pctFail = (c.fail / total * 100).toFixed(1);
  const pctReview = (c.review / total * 100).toFixed(1);

  const categories = categorizeFindings(findings);
  let gridHtml = '';
  for (const [label, items] of categories) {
    const dots = items.map(f =>
      `<span class="control-grid__dot ${statusToDotClass(f.status)}" title="${esc(f.control_id)}"></span>`
    ).join('');
    gridHtml += `<div class="control-grid__category"><span class="control-grid__title">${esc(label)}</span><div class="control-grid__dots">${dots}</div></div>`;
  }

  return `<div class="compliance-viz">
  <div class="compliance-viz__label">${c.pass} / ${total} controls passed</div>
  <div class="compliance-viz__bar">
    <div class="compliance-viz__fill compliance-viz__fill--pass" style="width:${pctPass}%"></div>
    <div class="compliance-viz__fill compliance-viz__fill--fail" style="width:${pctFail}%"></div>
    <div class="compliance-viz__fill compliance-viz__fill--review" style="width:${pctReview}%"></div>
  </div>
</div>
<div class="control-grid">${gridHtml}</div>`;
}

// ---- Dashboard data loading ----

async function loadDashboard() {
  // Fire all requests in parallel
  const [repoResult, prs, releases, auditEntries] = await Promise.allSettled([
    fetch(`/api/repos/${OWNER}/${REPO}/verify`).then(r => r.ok ? r.json() : null),
    swrFetch(`/api/repos/${OWNER}/${REPO}/pulls`),
    swrFetch(`/api/repos/${OWNER}/${REPO}/releases`),
    swrFetch(`/api/audit-history?owner=${OWNER}&repo=${REPO}&limit=8`),
  ]);

  // Repo verification → Compliance Viz + sidebar
  if (repoResult.status === 'fulfilled' && repoResult.value) {
    const data = repoResult.value;
    const profileName = data.profile_name || 'default';
    document.getElementById('compliance-viz-area').setHTML(renderComplianceViz(data.findings), _sanitizer);
    _lastRepoFindings = data.findings;
    _lastRepoProfile = profileName;
    const area = document.getElementById('result-area');
    area.setHTML(`<button class="btn--verify" id="show-findings-btn" data-action="show-repo-findings">検証結果を表示 (${countFindings(data.findings).pass + countFindings(data.findings).fail + countFindings(data.findings).review} 件)</button>`, _sanitizer);
  }

  // Build audit lookup for PR/Release verification status
  const auditMap = { pr: {}, release: {} };
  if (auditEntries.status === 'fulfilled') {
    for (const e of (auditEntries.value || [])) {
      if (e.type === 'pr' || e.type === 'release') {
        if (!auditMap[e.type][e.target_ref]) auditMap[e.type][e.target_ref] = e;
      }
    }
  }

  // PR card — primary metric: unverified count
  const prCountEl = document.getElementById('dash-pr-count');
  prCountEl.classList.remove('dash-card__skeleton');
  prCountEl.className = 'dash-card__count';
  if (prs.status === 'fulfilled') {
    const prList = prs.value || [];
    const openCount = prList.filter(p => !p.merged_at && p.state !== 'closed').length;
    const mergedCount = prList.filter(p => p.merged_at).length;
    const unverifiedCount = prList.filter(p => !auditMap.pr[String(p.pr_number)]).length;
    prCountEl.textContent = unverifiedCount;
    const prMetaEl = document.getElementById('dash-pr-meta');
    prMetaEl.textContent = `${unverifiedCount} 件未検証 / 全 ${prList.length} 件`;
    const prBadgesEl = document.getElementById('dash-pr-badges');
    let badges = '';
    if (openCount > 0) badges += `<span class="badge badge--review">${openCount} open</span>`;
    if (mergedCount > 0) badges += `<span class="badge badge--pass">${mergedCount} merged</span>`;
    prBadgesEl.setHTML(badges, _sanitizer);
    if (unverifiedCount === 0) document.getElementById('dash-pr').classList.add('dash-card--clear');
  } else {
    prCountEl.textContent = '—';
  }

  // Release card — primary metric: unverified count
  const relCountEl = document.getElementById('dash-release-count');
  relCountEl.classList.remove('dash-card__skeleton');
  relCountEl.className = 'dash-card__count';
  if (releases.status === 'fulfilled') {
    const relList = releases.value || [];
    const unverifiedRel = relList.filter(r => !auditMap.release[r.tag_name]).length;
    relCountEl.textContent = unverifiedRel;
    const relMetaEl = document.getElementById('dash-release-meta');
    relMetaEl.textContent = `${unverifiedRel} 件未検証 / 全 ${relList.length} 件`;
    if (relList.length > 0) {
      const latest = relList[0];
      const relBadgesEl = document.getElementById('dash-release-badges');
      const date = latest.published_at || latest.created_at;
      relBadgesEl.setHTML(`<span class="badge">${esc(latest.tag_name)}</span> <span class="release-item__badge-meta">${timeAgo(date)}</span>`, _sanitizer);
    }
    if (unverifiedRel === 0) document.getElementById('dash-release').classList.add('dash-card--clear');
  } else {
    relCountEl.textContent = '—';
  }

  // Recent activity
  if (auditEntries.status === 'fulfilled') {
    const entries = auditEntries.value || [];
    if (entries.length > 0) {
      const section = document.getElementById('activity-section');
      section.hidden = false;
      const list = document.getElementById('activity-list');
      list.setHTML(entries.map(e => {
        const typeLabel = e.type === 'pr' ? 'PR' : e.type === 'release' ? 'Release' : 'Repo';
        return `<div class="activity__item" data-audit-id="${e.id}" role="button" tabindex="0">
          <span class="activity__time">${timeAgo(e.verified_at + 'Z')}</span>
          <span class="type-badge type-${e.type}">${typeLabel}</span>
          <span class="activity__target">${esc(e.target_ref)}</span>
          <span class="activity__results">${compactBadges(e.pass, e.fail, e.review)}</span>
        </div>`;
      }).join(''), _sanitizer);
    }
  }
}

// ---- Repo verification ----

async function runVerify() {
  const btn = document.getElementById('verify-btn');
  const area = document.getElementById('result-area');
  const policyEl = document.getElementById('policy-select');
  const policy = policyEl.value;

  btn.disabled = true;
  btn.textContent = '検証中…';
  btn.classList.add('is-running');
  openSidebar('検証中…', '<div class="loading" role="status">検証を実行中</div>', '');

  try {
    const resp = await fetchWithTimeout(`/api/repos/${OWNER}/${REPO}/verify?policy=${encodeURIComponent(policy)}`, { method: 'POST' }, 60000);
    if (!resp.ok) throw new Error(await resp.text());
    const data = await resp.json();
    const profileName = data.profile_name || policy;
    _lastRepoFindings = data.findings;
    _lastRepoProfile = profileName;

    // Update compliance visualization
    document.getElementById('compliance-viz-area').setHTML(renderComplianceViz(data.findings), _sanitizer);

    // Show in sidebar
    openFindingsSidebar(`検証結果 — ${profileName}`, data.findings, {
      owner: OWNER, repo: REPO, target_ref: 'HEAD', policy: profileName,
    });

    const c = countFindings(data.findings);
    area.setHTML(`<button class="btn--verify" id="show-findings-btn" data-action="show-repo-findings">検証結果を表示 (${c.pass + c.fail + c.review} 件)</button>`, _sanitizer);
  } catch (e) {
    openSidebar('エラー', renderErrorCard(classifyError(e)), '');
  }

  btn.disabled = false;
  btn.textContent = '再検証';
  btn.classList.remove('is-running');
}

enhancePolicySelect(document.getElementById('policy-select'));
document.getElementById('verify-btn').addEventListener('click', runVerify);

// Show cached repo findings in sidebar
document.getElementById('result-area').addEventListener('click', function(e) {
  const btn = e.target.closest('[data-action="show-repo-findings"]');
  if (btn && _lastRepoFindings) {
    openFindingsSidebar(`検証結果 — ${_lastRepoProfile}`, _lastRepoFindings, {
      owner: OWNER, repo: REPO, target_ref: 'HEAD', policy: _lastRepoProfile,
    });
  }
});

// Activity item click → open audit in sidebar
document.getElementById('activity-list').addEventListener('click', function(e) {
  const item = e.target.closest('.activity__item');
  if (item && item.dataset.auditId) {
    openAuditInSidebar(Number(item.dataset.auditId));
  }
});

// ---- Load everything ----
loadDashboard();

(async function loadReadme() {
  const area = document.getElementById('readme-area');
  const loading = document.getElementById('readme-loading');
  try {
    const resp = await fetchWithTimeout(`/api/repos/${OWNER}/${REPO}/readme`);
    if (resp.status === 404) {
      area.hidden = true;
      return;
    }
    if (!resp.ok) throw new Error(await resp.text());
    const rawHtml = await resp.text();
    loading.remove();

    const header = document.createElement('div');
    header.className = 'readme__header';
    header.setHTML('<svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true"><path d="M0 1.75A.75.75 0 0 1 .75 1h4.253c1.227 0 2.317.59 3 1.501A3.743 3.743 0 0 1 11.006 1h4.245a.75.75 0 0 1 .75.75v10.5a.75.75 0 0 1-.75.75h-4.507a2.25 2.25 0 0 0-1.591.659l-.622.621a.75.75 0 0 1-1.06 0l-.622-.621A2.25 2.25 0 0 0 5.258 13H.75a.75.75 0 0 1-.75-.75Zm7.251 10.324.004-5.073-.002-2.253A2.25 2.25 0 0 0 5.003 2.5H1.5v9h3.757a3.75 3.75 0 0 1 1.994.574ZM8.755 4.75l-.004 7.322a3.752 3.752 0 0 1 1.992-.572H14.5v-9h-3.495a2.25 2.25 0 0 0-2.25 2.25Z"></path></svg> README.md', _sanitizer);
    area.appendChild(header);

    // Use base sanitizer for README — the Sanitizer API's default element
    // allowlist already covers all standard HTML elements including summary,
    // details, picture, source, figure, table, etc.
    // Note: specifying `elements` overrides defaults (whitelist-only), so we
    // must NOT set it to avoid stripping basic elements like p, a, div, etc.
    const content = document.createElement('div');
    content.className = 'markdown-body card';
    content.setHTML(rawHtml, _sanitizer);

    const ghRaw = `https://raw.githubusercontent.com/${OWNER}/${REPO}/HEAD/`;
    const ghBlob = `https://github.com/${OWNER}/${REPO}/blob/HEAD/`;

    content.querySelectorAll('img').forEach(img => {
      const src = img.getAttribute('src');
      if (src && !src.startsWith('http') && !src.startsWith('data:') && !src.startsWith('//')) {
        img.src = ghRaw + src.replace(/^\.?\//, '');
      }
      img.loading = 'lazy';
      img.decoding = 'async';
    });

    content.querySelectorAll('picture source').forEach(source => {
      const srcset = source.getAttribute('srcset');
      if (srcset && !srcset.startsWith('http') && !srcset.startsWith('data:')) {
        source.srcset = ghRaw + srcset.replace(/^\.?\//, '');
      }
    });

    content.querySelectorAll('a').forEach(a => {
      const href = a.getAttribute('href');
      if (!href) return;
      if (href.startsWith('#')) return;
      if (href.startsWith('http') || href.startsWith('//')) {
        a.target = '_blank';
        a.rel = 'noopener noreferrer';
        return;
      }
      a.href = ghBlob + href.replace(/^\.?\//, '');
      a.target = '_blank';
      a.rel = 'noopener noreferrer';
    });

    area.appendChild(content);
  } catch (e) {
    loading.remove();
    area.hidden = true;
  }
})();
