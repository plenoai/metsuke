/* Metsuke — Shared verification utilities */

const POLICY_DESCRIPTIONS = {
  'default': '汎用SDLCチェック — CI、コードレビュー、署名などの基本コントロールを検証',
  'oss': 'OSSプロジェクト向け — ライセンス、SECURITY.md、依存関係管理を重視',
  'aiops': 'AI/MLパイプライン向け — モデル管理、データリネージ、実験追跡を検証',
  'soc1': 'SOC 1 準拠 — 財務報告に関連するITコントロールを検証',
  'soc2': 'SOC 2 準拠 — セキュリティ・可用性・機密性のコントロールを検証',
  'slsa-l1': 'SLSA Level 1 — ビルドプロセスの文書化を検証',
  'slsa-l2': 'SLSA Level 2 — ホスト型ビルドサービスの使用を検証',
  'slsa-l3': 'SLSA Level 3 — ビルド環境の分離・改ざん防止を検証',
  'slsa-l4': 'SLSA Level 4 — 二者レビューとビルドの完全再現性を検証',
};

/**
 * Enhance a policy <select> element with descriptive title on change
 * and a companion help-text element.
 * @param {HTMLSelectElement} selectEl - The policy select element
 * @param {string} [helpElId] - Optional id of a <div> to show the description
 */
function enhancePolicySelect(selectEl, helpElId) {
  if (!selectEl) return;
  function update() {
    const desc = POLICY_DESCRIPTIONS[selectEl.value] || '';
    selectEl.title = desc;
    if (helpElId) {
      const el = document.getElementById(helpElId);
      if (el) el.textContent = desc;
    }
  }
  selectEl.addEventListener('change', update);
  update();
}

/**
 * Format an ISO date string as a relative time (e.g. "3日前", "2時間前").
 * Falls back to localized date if older than 30 days.
 */
function timeAgo(isoStr) {
  if (!isoStr) return '';
  const now = Date.now();
  const then = new Date(isoStr).getTime();
  const diff = now - then;
  const sec = Math.floor(diff / 1000);
  if (sec < 60) return 'たった今';
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}分前`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}時間前`;
  const day = Math.floor(hr / 24);
  if (day < 30) return `${day}日前`;
  return new Date(isoStr).toLocaleDateString('ja-JP');
}

/**
 * Render a skeleton loading placeholder for list views.
 * @param {number} count Number of skeleton rows
 */
function renderSkeleton(count = 4) {
  let items = '';
  for (let i = 0; i < count; i++) {
    items += `<div class="skeleton__item">
      <div class="skeleton__block">
        <div class="skeleton__line skeleton__line--title"></div>
        <div class="skeleton__line skeleton__line--meta"></div>
      </div>
      <div class="skeleton__line skeleton__line--short"></div>
    </div>`;
  }
  return `<div class="skeleton__list" role="status" aria-label="読み込み中">${items}</div>`;
}

function esc(s) {
  return (s || '').replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;');
}

function countFindings(outcomes) {
  let pass = 0, fail = 0, review = 0, na = 0;
  for (const o of (outcomes || [])) {
    if (o.decision === 'pass') pass++;
    else if (o.decision === 'fail') fail++;
    else if (o.decision === 'review') review++;
    else na++;
  }
  return { pass, fail, review, na };
}

function decisionBadge(decision) {
  if (decision === 'pass') return '<span class="badge badge--pass">PASS</span>';
  if (decision === 'fail') return '<span class="badge badge--fail">FAIL</span>';
  if (decision === 'review') return '<span class="badge badge--review">REVIEW</span>';
  return '<span class="badge badge--na">N/A</span>';
}

function renderFindingsTable(outcomes, titleHtml) {
  const counts = countFindings(outcomes);

  const rows = (outcomes || []).map(o => {
    const longText = (o.rationale || '').length > 120;
    const statusClass = o.decision === 'fail' ? 'is-violated' : o.decision === 'review' ? 'is-indeterminate' : o.decision === 'pass' ? 'is-satisfied' : '';
    if (longText) {
      return `<tr class="${statusClass}">
      <td class="findings__control-id">${esc(o.control_id)}</td>
      <td>${decisionBadge(o.decision)}</td>
      <td class="findings__rationale is-collapsible is-collapsed" role="button" tabindex="0" aria-expanded="false" data-action="toggle-rationale"><span class="findings__rationale-text">${esc(o.rationale)}</span><span class="findings__rationale-toggle"></span></td>
    </tr>`;
    }
    return `<tr class="${statusClass}">
      <td class="findings__control-id">${esc(o.control_id)}</td>
      <td>${decisionBadge(o.decision)}</td>
      <td class="findings__rationale">${esc(o.rationale)}</td>
    </tr>`;
  }).join('');

  return `
    <div class="findings">
      <div class="section__title">${titleHtml}</div>
      <div class="summary">
        <div class="summary__item"><span class="badge badge--pass">PASS</span> ${counts.pass}</div>
        <div class="summary__item"><span class="badge badge--review">REVIEW</span> ${counts.review}</div>
        <div class="summary__item"><span class="badge badge--fail">FAIL</span> ${counts.fail}</div>
        ${counts.na > 0 ? `<div class="summary__item"><span class="badge badge--na">N/A</span> ${counts.na}</div>` : ''}
      </div>
      <div class="card findings__card">
        <table class="findings__table" aria-label="検証結果">
          <thead>
            <tr><th scope="col">コントロール</th><th scope="col">ステータス</th><th scope="col">根拠</th></tr>
          </thead>
          <tbody>${rows}</tbody>
        </table>
      </div>
    </div>
  `;
}

function renderErrorCard(message) {
  return `<div class="card error-inline">
    <div class="error-inline__title">検証に失敗しました</div>
    <div class="error-inline__msg">${esc(message)}</div>
  </div>`;
}

function compactBadges(pass, fail, review) {
  let badges = '';
  if (pass > 0) badges += `<span class="badge badge--pass" title="PASS">PASS ${pass}</span>`;
  if (review > 0) badges += `<span class="badge badge--review" title="REVIEW">REVIEW ${review}</span>`;
  if (fail > 0) badges += `<span class="badge badge--fail" title="FAIL">FAIL ${fail}</span>`;
  return badges;
}

async function fetchWithTimeout(url, options = {}, timeoutMs = 30000) {
  const controller = new AbortController();
  const timeoutId = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const resp = await fetch(url, { ...options, signal: controller.signal });
    clearTimeout(timeoutId);
    return resp;
  } catch (e) {
    clearTimeout(timeoutId);
    if (e.name === 'AbortError') {
      throw new Error('リクエストがタイムアウトしました。サーバーの応答が遅延しています。');
    }
    throw e;
  }
}

function classifyError(err, resp) {
  if (!navigator.onLine || (err && err instanceof TypeError)) {
    return 'ネットワーク接続を確認してください。';
  }
  if (resp) {
    if (resp.status === 401 || resp.status === 403) return '認証の有効期限が切れました。ページを再読み込みしてください。';
    if (resp.status === 429) return 'リクエスト制限に達しました。しばらく待ってから再試行してください。';
    if (resp.status >= 500) return `サーバーエラー (${resp.status})。しばらく待ってから再試行してください。`;
  }
  return (err && err.message) || '不明なエラーが発生しました。';
}

function renderLoadError(containerId, message, retryFnName) {
  const el = document.getElementById(containerId);
  if (!el) return;
  el.setHTML(`<div class="empty-state">
    <div>${esc(message)}</div>
    <button class="btn--verify" data-action="retry" data-retry-fn="${retryFnName}">再取得</button>
  </div>`, _sanitizer);
}

// ---------------------------------------------------------------------------
// Sidebar — global result detail panel
// ---------------------------------------------------------------------------

function openSidebar(title, contentHtml, metaHtml) {
  const sidebar = document.getElementById('sidebar');
  const layout = document.getElementById('layout');
  const titleEl = document.getElementById('sidebar-title');
  const contentEl = document.getElementById('sidebar-content');

  titleEl.textContent = title;
  contentEl.setHTML(
    (metaHtml ? `<div class="sidebar__meta">${metaHtml}</div>` : '') + contentHtml,
    _sanitizer
  );

  sidebar.hidden = false;
  requestAnimationFrame(() => {
    sidebar.classList.add('is-open');
    layout.classList.add('has-sidebar');
  });
}

function closeSidebar() {
  const sidebar = document.getElementById('sidebar');
  const layout = document.getElementById('layout');
  sidebar.classList.remove('is-open');
  layout.classList.remove('has-sidebar');
  sidebar.addEventListener('transitionend', () => { sidebar.hidden = true; }, { once: true });
}

/**
 * Open sidebar with findings data.
 * @param {string} title - Sidebar title
 * @param {Array} findings - Array of finding objects
 * @param {object} [meta] - Optional metadata {type, owner, repo, target_ref, policy, verified_at, trigger}
 */
function openFindingsSidebar(title, findings, meta) {
  let metaHtml = '';
  if (meta) {
    const rows = [];
    if (meta.owner && meta.repo) {
      rows.push(`<div class="sidebar__meta-row"><span class="sidebar__meta-label">リポジトリ</span><span class="sidebar__meta-value">${esc(meta.owner)}/${esc(meta.repo)}</span></div>`);
    }
    if (meta.target_ref) {
      rows.push(`<div class="sidebar__meta-row"><span class="sidebar__meta-label">対象</span><span class="sidebar__meta-value">${esc(meta.target_ref)}</span></div>`);
    }
    if (meta.policy) {
      rows.push(`<div class="sidebar__meta-row"><span class="sidebar__meta-label">ポリシー</span><span class="sidebar__meta-value">${esc(meta.policy)}</span></div>`);
    }
    if (meta.verified_at) {
      rows.push(`<div class="sidebar__meta-row"><span class="sidebar__meta-label">日時</span><span class="sidebar__meta-value">${new Date(meta.verified_at + 'Z').toLocaleString('ja-JP')}</span></div>`);
    }
    if (meta.trigger) {
      rows.push(`<div class="sidebar__meta-row"><span class="sidebar__meta-label">トリガー</span><span class="sidebar__meta-value">${meta.trigger === 'webhook' ? 'auto' : '手動'}</span></div>`);
    }
    metaHtml = rows.join('');
  }

  const contentHtml = renderFindingsTable(findings, '');
  openSidebar(title, contentHtml, metaHtml);
}

/**
 * Load an audit entry by ID and show in sidebar.
 */
async function openAuditInSidebar(entryId) {
  openSidebar('読み込み中…', '<div class="loading" role="status">詳細を取得中</div>', '');
  try {
    const resp = await fetchWithTimeout(`/api/audit-history/${entryId}`);
    if (!resp.ok) throw new Error(await resp.text());
    const data = await resp.json();
    const typeLabel = data.type === 'pr' ? 'PR' : data.type === 'release' ? 'Release' : 'Repo';
    const title = `${typeLabel} — ${data.owner}/${data.repo}`;
    const outcomes = data.result && data.result.outcomes ? data.result.outcomes : [];
    openFindingsSidebar(title, outcomes, {
      owner: data.owner,
      repo: data.repo,
      target_ref: data.target_ref,
      policy: data.policy,
      verified_at: data.verified_at,
      trigger: data.trigger,
    });
  } catch (e) {
    openSidebar('エラー', renderErrorCard(classifyError(e)), '');
  }
}

// Sidebar close button + Escape key
document.getElementById('sidebar-close').addEventListener('click', closeSidebar);
document.addEventListener('keydown', function(e) {
  if (e.key === 'Escape' && document.getElementById('sidebar').classList.contains('is-open')) {
    closeSidebar();
  }
});

// Global event delegation for shared actions
document.addEventListener('click', function(e) {
  const toggle = e.target.closest('[data-action="toggle-rationale"]');
  if (toggle) {
    toggle.classList.toggle('is-collapsed');
    toggle.setAttribute('aria-expanded', !toggle.classList.contains('is-collapsed'));
    return;
  }
  const retry = e.target.closest('[data-action="retry"]');
  if (retry) {
    retry.disabled = true;
    retry.textContent = '再取得中…';
    const fn = window[retry.dataset.retryFn];
    if (fn) fn();
  }
});

document.addEventListener('keydown', function(e) {
  if (e.key === 'Enter' || e.key === ' ') {
    const toggle = e.target.closest('[data-action="toggle-rationale"]');
    if (toggle) {
      e.preventDefault();
      toggle.click();
    }
  }
});
