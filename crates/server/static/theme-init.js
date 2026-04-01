// Apply saved theme + matching markdown CSS before first paint.
// Must be loaded synchronously in <head> to prevent FOUC.
(function() {
  var t = localStorage.getItem('metsuke-theme');
  if (!t) return;
  document.documentElement.setAttribute('data-theme', t);
  if (t === 'github-light' || t === 'github-light-hc') {
    var link = document.getElementById('github-markdown-css');
    if (link) {
      link.removeAttribute('integrity');
      link.href = 'https://cdn.jsdelivr.net/npm/github-markdown-css@5.9.0/github-markdown-light.min.css';
    }
  }
})();
