/* Metsuke — Shared verification utilities */

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
    return `<tr class="status-${f.status}">
      <td style="white-space:nowrap">${esc(f.control_id)}</td>
      <td>${statusBadge(f.status)}</td>
      <td class="rationale-cell${longText ? ' collapsed' : ''}" ${longText ? 'role="button" tabindex="0" aria-expanded="false" onclick="this.classList.toggle(\'collapsed\');this.setAttribute(\'aria-expanded\',!this.classList.contains(\'collapsed\'))" onkeydown="if(event.key===\'Enter\'||event.key===\' \'){event.preventDefault();this.click()}"' : ''}>${esc(f.rationale)}</td>
    </tr>`;
  }).join('');

  return `
    <div style="margin-top:1.5rem">
      <div class="section-title">${titleHtml}</div>
      <div class="summary-bar">
        <div class="summary-item"><span class="badge badge-pass">PASS</span> ${counts.pass}</div>
        <div class="summary-item"><span class="badge badge-review">REVIEW</span> ${counts.review}</div>
        <div class="summary-item"><span class="badge badge-fail">FAIL</span> ${counts.fail}</div>
        ${counts.na > 0 ? `<div class="summary-item"><span class="badge badge-na">N/A</span> ${counts.na}</div>` : ''}
      </div>
      <div class="card" style="padding:0;overflow-x:auto">
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
  return `<div class="card" style="border-color:var(--accent-vermillion)">
    <div style="font-family:var(--font-mono);font-size:0.8rem;color:var(--accent-vermillion);margin-bottom:0.5rem">検証に失敗しました</div>
    <div style="font-family:var(--font-mono);font-size:0.72rem;color:var(--text-secondary)">${esc(message)}</div>
  </div>`;
}

function compactBadges(pass, fail, review) {
  let badges = '';
  if (pass > 0) badges += `<span class="badge badge-pass" title="PASS">PASS ${pass}</span>`;
  if (review > 0) badges += `<span class="badge badge-review" title="REVIEW">REV ${review}</span>`;
  if (fail > 0) badges += `<span class="badge badge-fail" title="FAIL">FAIL ${fail}</span>`;
  return badges;
}
