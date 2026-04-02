function copyText(id, btn) {
  const t = document.getElementById(id).textContent;
  navigator.clipboard.writeText(t).then(() => {
    btn.textContent = 'COPIED';
    btn.classList.add('is-copied');
    setTimeout(() => { btn.textContent = 'COPY'; btn.classList.remove('is-copied'); }, 1500);
  });
}
document.getElementById('copy-config-btn').addEventListener('click', function() { copyText('config', this); });
