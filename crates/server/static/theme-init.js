// Apply saved theme + matching markdown CSS before first paint.
// Must be loaded synchronously in <head> to prevent FOUC.
(function() {
  var t = localStorage.getItem('metsuke-theme') || 'github-dark';
  document.documentElement.setAttribute('data-theme', t);
  if (t === 'github-light' || t === 'github-light-hc') {
    var link = document.getElementById('github-markdown-css');
    if (link) {
      link.href = '/static/vendor/github-markdown-light.min.css';
    }
  }
})();
