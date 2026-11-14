/* Buyer-side behaviour. The bag is real state, so adding, removing and the
   header count all behave the way a shopper expects. */
(function () {
  'use strict';

  var $ = function (s, r) { return (r || document).querySelector(s); };
  var $$ = function (s, r) { return [].slice.call((r || document).querySelectorAll(s)); };
  var money = function (n) { return '$' + n.toFixed(2); };
  var KEY = 'reloop.bag';

  /* ------------------------------------------------------------- bag state */
  function read() {
    try { return JSON.parse(sessionStorage.getItem(KEY)) || []; } catch (e) { return []; }
  }
  function write(items) {
    try { sessionStorage.setItem(KEY, JSON.stringify(items)); } catch (e) {}
    paintCount();
  }
  function add(item) { var b = read(); b.push(item); write(b); }

  function paintCount() {
    var n = read().length;
    $$('[data-bag-count]').forEach(function (el) { el.textContent = 'BAG [' + n + ']'; });
  }

  /* --------------------------------------------------------------- toasts */
  function toast(msg) {
    var t = document.createElement('div');
    t.className = 'toast';
    t.textContent = msg;
    document.body.appendChild(t);
    requestAnimationFrame(function () { t.classList.add('is-in'); });
    setTimeout(function () { t.classList.remove('is-in'); setTimeout(function () { t.remove(); }, 300); }, 2200);
  }

  /* ----------------------------------------------------------------- PDP */
  function pdp() {
    var panel = $('[data-pdp]');
    if (!panel) return;
    var chosen = null;

    $$('.size-row:not(.is-out)').forEach(function (row) {
      row.addEventListener('click', function () {
        $$('.size-row').forEach(function (r) { r.classList.remove('is-selected'); });
        row.classList.add('is-selected');
        chosen = row.getAttribute('data-size');
        var btn = $('[data-add]');
        if (btn) { btn.classList.remove('btn--disabled');  }
        var hint = $('[data-size-hint]');
        if (hint) hint.textContent = '';
      });
    });

    var add_ = $('[data-add]');
    if (!add_) return;
    add_.addEventListener('click', function () {
      var needsSize = panel.getAttribute('data-needs-size') !== 'false';
      if (needsSize && !chosen) {
        var hint = $('[data-size-hint]');
        if (hint) hint.textContent = 'Please select a size.';
        $$('.size-row:not(.is-out)').forEach(function (r) {
          r.classList.add('is-flash');
          setTimeout(function () { r.classList.remove('is-flash'); }, 700);
        });
        return;
      }
      add({
        name: panel.getAttribute('data-name'),
        colour: panel.getAttribute('data-colour'),
        price: parseFloat(panel.getAttribute('data-price')),
        img: panel.getAttribute('data-img'),
        ph: panel.getAttribute('data-ph'),
        size: chosen || panel.getAttribute('data-size-fixed') || 'M',
        tier: panel.getAttribute('data-tier') || '',
        seller: panel.getAttribute('data-seller') || '',
        eta: panel.getAttribute('data-eta') || '',
        preowned: panel.getAttribute('data-preowned') === 'true'
      });
      toast('Added to your bag');
      setTimeout(function () { window.location.href = 'bag.html'; }, 550);
    });
  }

  /* ----------------------------------------------------------------- BAG */
  function bag() {
    var root = $('[data-bag]');
    if (!root) return;
    var items = read();

    if (!items.length) {
      root.innerHTML = '<div class="bag-empty">' +
        '<p class="u-label">YOUR BAG IS EMPTY</p>' +
        '<p class="u-meta">Add something from the blazers page, or try the sold-out size.</p>' +
        '<a class="btn btn--primary" href="plp.html">CONTINUE SHOPPING</a></div>';
      var sum = $('[data-sum]'); if (sum) sum.hidden = true;
      return;
    }

    root.innerHTML = items.map(function (it, i) {
      return '<div class="bag__item">' +
        '<div class="bag__media">' + (it.img
          ? '<img src="assets/img/' + it.img + '.webp" alt="">'
          : '<div class="ph ph--' + (it.ph || 'a') + '"></div>') + '</div>' +
        '<div class="bag__meta">' +
          (it.preowned ? '<p class="u-eyebrow">ZARA PRE-OWNED</p>' : '') +
          '<p class="bag__name">' + it.name + ' - ' + it.colour + '</p>' +
          '<p class="u-meta">Size ' + it.size + (it.tier ? ' &middot; ' + it.tier : '') + (it.seller ? ' &middot; ' + it.seller : '') + '</p>' +
          (it.eta ? '<p class="u-meta">' + it.eta + '</p>' : '') +
          '<div class="bag__links"><button class="link-u" data-remove="' + i + '">Remove</button></div>' +
        '</div>' +
        '<div class="bag__price">' + money(it.price) + '</div>' +
      '</div>';
    }).join('');

    var sub = items.reduce(function (a, b) { return a + b.price; }, 0);
    var anyPre = items.some(function (i) { return i.preowned; });
    var fee = anyPre ? 1 + sub * 0.05 : 0;
    var ship = 4.95;

    var set = function (sel, v) { var e = $(sel); if (e) e.textContent = v; };
    set('[data-sub]', money(sub));
    set('[data-fee]', money(fee));
    set('[data-ship]', money(ship));
    set('[data-total]', money(sub + fee + ship));
    var feeRow = $('[data-fee-row]'); if (feeRow) feeRow.hidden = !anyPre;
    var feeNote = $('[data-fee-note]'); if (feeNote) feeNote.hidden = !anyPre;

    root.addEventListener('click', function (e) {
      var b = e.target.closest && e.target.closest('[data-remove]');
      if (!b) return;
      var all = read();
      all.splice(parseInt(b.getAttribute('data-remove'), 10), 1);
      write(all);
      bag();
    });
  }

  /* ------------------------------------------------------------ walkthrough */
  function keys() {
    document.addEventListener('keydown', function (e) {
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      var t = e.target;
      if (t && /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName)) return;
      var sel = e.key === 'ArrowRight' ? '[data-wt-next]' : e.key === 'ArrowLeft' ? '[data-wt-prev]' : null;
      if (!sel) return;
      var a = $(sel);
      if (a) { e.preventDefault(); window.location.href = a.getAttribute('href'); }
    });
  }

  /* --------------------------------------------------------------- misc UI */
  function misc() {
    // the sold-out size opens the pre-owned sheet
    document.addEventListener('click', function (e) {
      var o = e.target.closest && e.target.closest('[data-open-sheet]');
      if (o) { e.preventDefault(); window.location.href = 'pdp-reloop.html'; return; }
      var c = e.target.closest && e.target.closest('[data-close-sheet]');
      if (c) { e.preventDefault(); window.location.href = 'pdp.html'; }
    });

    // accordions
    $$('.acc__body').forEach(function (b) { b.style.display = 'none'; });
    document.addEventListener('click', function (e) {
      var row = e.target.closest && e.target.closest('.acc__row');
      if (!row) return;
      var body = row.nextElementSibling;
      if (!body || !body.classList.contains('acc__body')) return;
      var hidden = body.style.display === 'none';
      body.style.display = hidden ? 'block' : 'none';
      var ico = row.querySelector('.acc__ico');
      if (ico) ico.textContent = hidden ? '−' : '+';
    });

    // colour swatches
    $$('.swatch').forEach(function (s) {
      s.addEventListener('click', function () {
        $$('.swatch').forEach(function (x) { x.setAttribute('aria-checked', 'false'); });
        s.setAttribute('aria-checked', 'true');
      });
    });

    // condition filter chips on the pre-owned catalogue
    $$('.po-filters .chip').forEach(function (c) {
      c.addEventListener('click', function () {
        c.classList.toggle('is-on');
        var on = $$('.po-filters .chip.is-on').map(function (x) { return x.textContent.trim(); });
        $$('.grid .card').forEach(function (card) {
          var flag = card.querySelector('.card__flag');
          if (!flag) return;
          card.style.display = (!on.length || on.indexOf(flag.textContent.trim()) > -1) ? '' : 'none';
        });
      });
    });
  }

  document.addEventListener('DOMContentLoaded', function () {
    paintCount(); pdp(); bag(); keys(); misc();
  });
})();
