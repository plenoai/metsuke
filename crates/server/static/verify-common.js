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

function esc(s) {
  return (s || '').replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;');
}

function countFindings(findings) {
  let pass = 0, fail = 0, review = 0, na = 0;
  for (const f of (findings || [])) {
    if (f.status === 'satisfied') pass++;
    else if (f.status === 'violated') fail++;
    else if (f.status === 'indeterminate') review++;
    else na++;
  }
  return { pass, fail, review, na };
}

function statusBadge(status) {
  if (status === 'satisfied') return '<span class="badge badge-pass">PASS</span>';
  if (status === 'violated') return '<span class="badge badge-fail">FAIL</span>';
  if (status === 'indeterminate') return '<span class="badge badge-review">REVIEW</span>';
  if (status === 'not_applicable') return '<span class="badge badge-na">N/A</span>';
  return '<span class="badge">' + esc(status) + '</span>';
}

function renderFindingsTable(findings, titleHtml) {
  const counts = countFindings(findings);

  const rows = (findings || []).map(f => {
    const longText = (f.rationale || '').length > 120;
    if (longText) {
      return `<tr class="status-${f.status}">
      <td style="white-space:nowrap">${esc(f.control_id)}</td>
      <td>${statusBadge(f.status)}</td>
      <td class="rationale-cell collapsible collapsed" role="button" tabindex="0" aria-expanded="false" data-action="toggle-rationale"><span class="rationale-text">${esc(f.rationale)}</span><span class="rationale-toggle"></span></td>
    </tr>`;
    }
    return `<tr class="status-${f.status}">
      <td style="white-space:nowrap">${esc(f.control_id)}</td>
      <td>${statusBadge(f.status)}</td>
      <td class="rationale-cell">${esc(f.rationale)}</td>
    </tr>`;
  }).join('');

  return `
    <div class="findings-section">
      <div class="section-title">${titleHtml}</div>
      <div class="summary-bar">
        <div class="summary-item"><span class="badge badge-pass">PASS</span> ${counts.pass}</div>
        <div class="summary-item"><span class="badge badge-review">REVIEW</span> ${counts.review}</div>
        <div class="summary-item"><span class="badge badge-fail">FAIL</span> ${counts.fail}</div>
        ${counts.na > 0 ? `<div class="summary-item"><span class="badge badge-na">N/A</span> ${counts.na}</div>` : ''}
      </div>
      <div class="card findings-card">
        <table class="findings-table" aria-label="検証結果">
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
  return `<div class="card error-card-inline">
    <div class="error-card-title">検証に失敗しました</div>
    <div class="error-card-message">${esc(message)}</div>
  </div>`;
}

function compactBadges(pass, fail, review) {
  let badges = '';
  if (pass > 0) badges += `<span class="badge badge-pass" title="PASS">PASS ${pass}</span>`;
  if (review > 0) badges += `<span class="badge badge-review" title="REVIEW">REVIEW ${review}</span>`;
  if (fail > 0) badges += `<span class="badge badge-fail" title="FAIL">FAIL ${fail}</span>`;
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
  el.setHTML(`<div class="empty-state load-error">
    <div>${esc(message)}</div>
    <button class="verify-btn" data-action="retry" data-retry-fn="${retryFnName}">再取得</button>
  </div>`, _sanitizer);
}

// Global event delegation for shared actions
document.addEventListener('click', function(e) {
  const toggle = e.target.closest('[data-action="toggle-rationale"]');
  if (toggle) {
    toggle.classList.toggle('collapsed');
    toggle.setAttribute('aria-expanded', !toggle.classList.contains('collapsed'));
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
