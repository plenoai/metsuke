const OWNER = document.currentScript.dataset.owner;
const REPO = document.currentScript.dataset.repo;

// ---- Dashboard data loading ----

async function loadDashboard() {
  // Fire all requests in parallel
  const [repoResult, prs, releases, auditEntries] = await Promise.allSettled([
    fetch(`/api/repos/${OWNER}/${REPO}/verify`).then(r => r.ok ? r.json() : null),
    swrFetch(`/api/repos/${OWNER}/${REPO}/pulls`),
    swrFetch(`/api/repos/${OWNER}/${REPO}/releases`),
    swrFetch(`/api/audit-history?owner=${OWNER}&repo=${REPO}&limit=8`),
  ]);

  // Repo verification card
  const repoCountEl = document.getElementById('dash-repo-count');
  const repoBadgesEl = document.getElementById('dash-repo-badges');
  const repoMetaEl = document.getElementById('dash-repo-meta');
  repoCountEl.classList.remove('dash-card-skeleton');

  if (repoResult.status === 'fulfilled' && repoResult.value) {
    const data = repoResult.value;
    const c = countFindings(data.findings);
    const total = c.pass + c.fail + c.review + c.na;
    repoCountEl.textContent = '';
    repoCountEl.className = 'dash-card-count';
    repoCountEl.textContent = `${c.pass}/${total}`;
    repoBadgesEl.setHTML(compactBadges(c.pass, c.fail, c.review), _sanitizer);
    const profileName = data.profile_name || 'default';
    repoMetaEl.textContent = `policy: ${profileName}`;

    // Also show in result area
    const area = document.getElementById('result-area');
    area.setHTML(renderFindingsTable(data.findings, `検証結果 — ${esc(profileName)}`), _sanitizer);
  } else {
    repoCountEl.className = 'dash-card-count';
    repoCountEl.textContent = '';
    document.getElementById('dash-repo').classList.add('dash-card--empty');
    repoCountEl.textContent = '未検証';
    repoMetaEl.textContent = '検証ボタンで開始';
  }

  // PR card
  const prCountEl = document.getElementById('dash-pr-count');
  prCountEl.classList.remove('dash-card-skeleton');
  prCountEl.className = 'dash-card-count';
  if (prs.status === 'fulfilled') {
    const prList = prs.value || [];
    const openCount = prList.filter(p => !p.merged_at && p.state !== 'closed').length;
    const mergedCount = prList.filter(p => p.merged_at).length;
    prCountEl.textContent = prList.length;
    const prBadgesEl = document.getElementById('dash-pr-badges');
    let meta = '';
    if (openCount > 0) meta += `<span class="badge badge-pass">${openCount} open</span>`;
    if (mergedCount > 0) meta += `<span class="badge badge-private">${mergedCount} merged</span>`;
    prBadgesEl.setHTML(meta, _sanitizer);
  } else {
    prCountEl.textContent = '—';
  }

  // Release card
  const relCountEl = document.getElementById('dash-release-count');
  relCountEl.classList.remove('dash-card-skeleton');
  relCountEl.className = 'dash-card-count';
  if (releases.status === 'fulfilled') {
    const relList = releases.value || [];
    relCountEl.textContent = relList.length;
    if (relList.length > 0) {
      const latest = relList[0];
      const relBadgesEl = document.getElementById('dash-release-badges');
      const date = latest.published_at || latest.created_at;
      relBadgesEl.setHTML(`<span class="badge">${esc(latest.tag_name)}</span> <span class="release-badge-meta">${timeAgo(date)}</span>`, _sanitizer);
    }
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
        return `<div class="activity-item">
          <span class="activity-time">${timeAgo(e.verified_at + 'Z')}</span>
          <span class="type-badge type-${e.type}">${typeLabel}</span>
          <span class="activity-target">${esc(e.target_ref)}</span>
          <span class="activity-results">${compactBadges(e.pass, e.fail, e.review)}</span>
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
  btn.classList.add('running');
  area.setHTML('<div class="loading" role="status">検証を実行中</div>', _sanitizer);

  try {
    const resp = await fetchWithTimeout(`/api/repos/${OWNER}/${REPO}/verify?policy=${encodeURIComponent(policy)}`, { method: 'POST' }, 60000);
    if (!resp.ok) throw new Error(await resp.text());
    const data = await resp.json();
    const profileName = data.profile_name || policy;
    area.setHTML(renderFindingsTable(data.findings, `検証結果 — ${esc(profileName)}`), _sanitizer);

    // Update dashboard card
    const c = countFindings(data.findings);
    const total = c.pass + c.fail + c.review + c.na;
    const repoCountEl = document.getElementById('dash-repo-count');
    repoCountEl.className = 'dash-card-count';
    repoCountEl.textContent = `${c.pass}/${total}`;
    document.getElementById('dash-repo').classList.remove('dash-card--empty');
    document.getElementById('dash-repo-badges').setHTML(compactBadges(c.pass, c.fail, c.review), _sanitizer);
    document.getElementById('dash-repo-meta').textContent = `policy: ${profileName}`;
  } catch (e) {
    area.setHTML(renderErrorCard(e.message), _sanitizer);
  }

  btn.disabled = false;
  btn.textContent = '再検証';
  btn.classList.remove('running');
}

enhancePolicySelect(document.getElementById('policy-select'));
document.getElementById('verify-btn').addEventListener('click', runVerify);

// ---- Load everything ----
loadDashboard();

(async function loadReadme() {
  const area = document.getElementById('readme-area');
  const loading = document.getElementById('readme-loading');
  try {
    const resp = await fetch(`/api/repos/${OWNER}/${REPO}/readme`);
    if (resp.status === 404) {
      area.hidden = true;
      return;
    }
    if (!resp.ok) throw new Error(await resp.text());
    const rawHtml = await resp.text();
    loading.remove();

    const header = document.createElement('div');
    header.className = 'readme-header';
    header.setHTML('<svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true"><path d="M0 1.75A.75.75 0 0 1 .75 1h4.253c1.227 0 2.317.59 3 1.501A3.743 3.743 0 0 1 11.006 1h4.245a.75.75 0 0 1 .75.75v10.5a.75.75 0 0 1-.75.75h-4.507a2.25 2.25 0 0 0-1.591.659l-.622.621a.75.75 0 0 1-1.06 0l-.622-.621A2.25 2.25 0 0 0 5.258 13H.75a.75.75 0 0 1-.75-.75Zm7.251 10.324.004-5.073-.002-2.253A2.25 2.25 0 0 0 5.003 2.5H1.5v9h3.757a3.75 3.75 0 0 1 1.994.574ZM8.755 4.75l-.004 7.322a3.752 3.752 0 0 1 1.992-.572H14.5v-9h-3.495a2.25 2.25 0 0 0-2.25 2.25Z"></path></svg> README.md', _sanitizer);
    area.appendChild(header);

    // README sanitizer: extends base _sanitizer with markdown-specific elements/attributes
    const readmeSanitizer = { sanitizer: new Sanitizer({
      attributes: [
        ..._sanitizer.sanitizer.get().attributes,
        'align', 'media', 'srcset', 'sizes', 'loading', 'decoding',
        'crossorigin', 'mathvariant', 'encoding', 'colspan', 'rowspan',
        'datetime', 'open', 'start', 'reversed',
      ],
      elements: [
        'summary', 'details', 'picture', 'source', 'figcaption', 'figure',
        'math', 'mrow', 'mi', 'mo', 'mn', 'msup', 'msub', 'mfrac',
        'mover', 'munder', 'mspace', 'mtable', 'mtr', 'mtd',
        'annotation', 'semantics',
      ],
    }) };

    const content = document.createElement('div');
    content.className = 'markdown-body card';
    content.setHTML(rawHtml, readmeSanitizer);

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
