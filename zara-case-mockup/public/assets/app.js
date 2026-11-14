/* Presenter behaviour. The mockup is deliberately non-functional except for
   the few interactions the walkthrough depends on. */
(function () {
  'use strict';

  // Arrow keys drive the walkthrough — hands-free on stage.
  document.addEventListener('keydown', function (e) {
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    var t = e.target;
    if (t && /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName)) return;
    var sel = e.key === 'ArrowRight' ? '[data-wt-next]' : e.key === 'ArrowLeft' ? '[data-wt-prev]' : null;
    if (!sel) return;
    var a = document.querySelector(sel);
    if (a) { e.preventDefault(); window.location.href = a.getAttribute('href'); }
  });

  // The sold-out size opens the pre-owned sheet.
  document.addEventListener('click', function (e) {
    var open = e.target.closest && e.target.closest('[data-open-sheet]');
    if (open) { e.preventDefault(); window.location.href = 'pdp-reloop.html'; return; }
    var close = e.target.closest && e.target.closest('[data-close-sheet]');
    if (close) { e.preventDefault(); window.location.href = 'pdp.html'; }
  });

  // Accordions.
  document.addEventListener('click', function (e) {
    var row = e.target.closest && e.target.closest('.acc__row');
    if (!row) return;
    var body = row.nextElementSibling;
    var ico = row.querySelector('.acc__ico');
    if (!body || !body.classList.contains('acc__body')) return;
    var hidden = body.style.display === 'none' || !body.dataset.init;
    body.dataset.init = '1';
    body.style.display = hidden ? 'block' : 'none';
    if (ico) ico.textContent = hidden ? '−' : '+';
  });

  // Accordion bodies start closed.
  document.querySelectorAll('.acc__body').forEach(function (b) { b.style.display = 'none'; });

  // Size rows select.
  document.addEventListener('click', function (e) {
    var row = e.target.closest && e.target.closest('.size-row:not(.is-out)');
    if (!row) return;
    row.parentNode.querySelectorAll('.size-row').forEach(function (r) { r.classList.remove('is-selected'); });
    row.classList.add('is-selected');
  });
})();
