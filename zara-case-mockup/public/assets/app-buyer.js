/* ZARA iOS — buyer. Pre-owned stands beside new on every product. */
(function () {
  'use strict';
  var $ = function (s, r) { return (r || document).querySelector(s); };
  var $$ = function (s, r) { return [].slice.call((r || document).querySelectorAll(s)); };
  var money = function (n) { return '$' + n.toFixed(2); };

  var TIERS = {
    e: { code: 'EXCELLENT', blurb: 'Worn a handful of times. No visible wear.' },
    g: { code: 'GOOD',      blurb: 'Light, even wear. Shape and colour intact.' },
    f: { code: 'FAIR',      blurb: 'Visible wear, photographed in detail.' }
  };
  var CHECKS = [
    ['Pilling & surface', 'No pilling detected'],
    ['Seams & structure', 'Intact, shoulder line true'],
    ['Staining & marks', 'None detected'],
    ['Hardware & fastenings', 'All buttons present']
  ];

  /* Catalogue — six distinct garments. */
  var CAT = [
    { id:'blazer', name:'TEXTURED TWEED BLAZER', col:'Blue', price:109, img:'blazer', alt:'blazer-2',
      ref:'2757/397/403', desc:'Regular-fit blazer with a lapel collar, long sleeves, front welt pockets and frayed trims.' },
    { id:'dress',  name:'DRAPED MIDI DRESS',    col:'Black', price:69.9, img:'dress', ref:'5644/024/800',
      desc:'Midi dress with a round neck, short sleeves and draped detail at the side.' },
    { id:'black',  name:'CROPPED KNIT SWEATER', col:'Black', price:45.9, img:'black', ref:'9598/293/800',
      desc:'Cropped sweater with a round neck and long sleeves.' },
    { id:'knit',   name:'CROCHET KNIT TOP',     col:'Oyster white', price:49.9, img:'knit', ref:'3991/012/712',
      desc:'Short sleeve top with a round neck and ruffled trim.' },
    { id:'mini',   name:'PLEATED MINI SKIRT',   col:'Grey marl', price:45.9, img:'mini', ref:'2731/313/330',
      desc:'Mini skirt with box pleats and a concealed side zip.' },
    { id:'skirt',  name:'FRINGED STRIPED SKIRT',col:'Multicoloured', price:89.9, img:'skirt', ref:'2731/313/331',
      desc:'Long skirt with an elastic waistband and fringed hem.' }
  ];

  /* Pre-owned listings. Same SKU = same garment, different owners' photos. */
  var PO = [
    { of:'blazer', tier:'e', price:82, img:'po-blazer-e', seller:'Brooklyn, NY',     when:'March 2026',    eta:'24–27 Nov' },
    { of:'blazer', tier:'g', price:63, img:'po-blazer-g', seller:'Queens, NY',       when:'November 2025', eta:'25–28 Nov' },
    { of:'blazer', tier:'f', price:46, img:'po-blazer-f', seller:'Jersey City, NJ',  when:'August 2025',   eta:'25–28 Nov' },
    { of:'dress',  tier:'g', price:41, img:'po-dress-g',  seller:'Hoboken, NJ',      when:'February 2026', eta:'25–28 Nov' },
    { of:'black',  tier:'f', price:19, img:'po-black-f',  seller:'Astoria, NY',      when:'July 2025',     eta:'26–29 Nov' },
    { of:'knit',   tier:'e', price:34, img:'po-knit-e',   seller:'Park Slope, NY',   when:'May 2026',      eta:'24–27 Nov' },
    { of:'mini',   tier:'g', price:27, img:'po-mini-g',   seller:'Harlem, NY',       when:'October 2025',  eta:'25–28 Nov' },
    { of:'skirt',  tier:'f', price:38, img:'po-skirt-f',  seller:'Bushwick, NY',     when:'June 2025',     eta:'26–29 Nov' }
  ];

  var SIZES = [
    { s:'XS', state:'in' }, { s:'S', state:'in' }, { s:'M', state:'out' },
    { s:'L', state:'in' },  { s:'XL', state:'soon' }
  ];

  var product = CAT[0], listing = null, bag = [], hist = [];
  var itemOf = function (id) { return CAT.filter(function (c) { return c.id === id; })[0]; };
  var poFor  = function (id) { return PO.filter(function (p) { return p.of === id; }); };

  /* ------------------------------------------------------------ routing */
  function go(v, push) {
    if (push !== false) { var cur = $('.view.is-on'); if (cur) hist.push(cur.id.slice(2)); }
    $$('.view').forEach(function (s) { s.classList.toggle('is-on', s.id === 'v-' + v); });
    closeSheets();
    var sc = $('#v-' + v + ' .scroll'); if (sc) sc.scrollTop = 0;
  }
  function back() { var p = hist.pop(); if (p) go(p, false); }

  function openSheet(n) {
    $('#sheetbg').classList.add('is-on');
    $('#sheet-' + n).classList.add('is-on');
  }
  function closeSheets() {
    $('#sheetbg').classList.remove('is-on');
    $$('.sheet').forEach(function (s) { s.classList.remove('is-on'); });
  }

  function toast(m) {
    var t = $('#toast'); t.textContent = m; t.classList.add('is-on');
    setTimeout(function () { t.classList.remove('is-on'); }, 1900);
  }

  /* -------------------------------------------------------------- views */
  function paintPLP() {
    $('#plpgrid').innerHTML = CAT.map(function (p) {
      return '<button class="tile" data-open="' + p.id + '">' +
        '<span class="tile__m"><img src="assets/img/' + p.img + '.webp" alt=""></span>' +
        '<span class="tile__i"><span class="tile__n">' + p.name + '</span>' +
        '<span class="tile__p">' + money(p.price) + '</span></span></button>';
    }).join('');
  }

  function openProduct(id) {
    product = itemOf(id);
    $('#pdp-img').src = 'assets/img/' + product.img + '.webp';
    $('#pdp-img2').src = 'assets/img/' + (product.alt || product.img) + '.webp';
    $('#pdp-name').textContent = product.name + ' - ' + product.col;
    $('#pdp-price').textContent = money(product.price);
    $('#pdp-ref').textContent = product.col + ' ' + product.ref;
    $('#pdp-desc').textContent = product.desc;
    var po = poFor(product.id);
    var from = po.length ? Math.min.apply(null, po.map(function (p) { return p.price; })) : null;
    $('#pdp-pofrom').textContent = from ? 'FROM ' + money(from) : 'NONE YET';
    go('pdp');
  }

  function paintSizes() {
    $('#sizelist').innerHTML = SIZES.map(function (z) {
      if (z.state === 'in')   return '<button class="srow" data-size="' + z.s + '"><span class="srow__n">' + z.s + '</span></button>';
      if (z.state === 'soon') return '<button class="srow"><span class="srow__n">' + z.s + '</span><span class="srow__m">Coming soon · <u>Notify me</u></span></button>';
      // sold out: the highest-intent moment to offer pre-owned
      return '<button class="srow is-out has-alt" data-pojump><span class="srow__n">' + z.s + '</span>' +
             '<span class="srow__alt">PRE-OWNED IN M</span></button>';
    }).join('');
  }

  function paintUnits() {
    var po = poFor(product.id);
    $('#posheet-h').textContent = 'PRE-OWNED · ' + product.name;
    $('#posheet-sub').textContent = po.length
      ? po.length + ' people already own this and will sell it. Graded and priced by Zara.'
      : 'Nobody has listed this one yet. We will notify you when someone does.';
    $('#units').innerHTML = po.map(function (u, i) {
      var t = TIERS[u.tier];
      return '<button class="unit" data-unit="' + i + '">' +
        '<span class="unit__m"><img src="assets/img/' + u.img + '.webp" alt=""></span>' +
        '<span><span class="unit__t"><span class="dot dot--' + u.tier + '"></span>' + t.code + '</span>' +
        '<span class="unit__p" style="display:block">' + money(u.price) +
        ' <span class="u-str">' + money(product.price) + '</span></span>' +
        '<span class="unit__s" style="display:block">' + t.blurb + '<br>' + u.seller + ' · ' + u.eta + '</span></span>' +
      '</button>';
    }).join('');
  }

  function openListing(u) {
    listing = u;
    var p = itemOf(u.of), t = TIERS[u.tier];
    $('#po-img').src = 'assets/img/' + u.img + '.webp';
    $('#po-img2').src = 'assets/img/' + p.img + '.webp';
    $('#po-name').textContent = p.name + ' - ' + p.col;
    $('#po-price').textContent = money(u.price);
    $('#po-was').textContent = money(p.price);
    $('#po-ref').textContent = p.col + ' ' + p.ref + ' · Size M';
    $('#po-dot').className = 'dot dot--' + u.tier;
    $('#po-tier').textContent = t.code;
    $('#po-blurb').textContent = t.blurb;
    $('#po-checks').innerHTML = CHECKS.map(function (c) {
      return '<div class="check"><span class="check__k">' + c[0] + '</span><span class="check__v">' + c[1] + '</span></div>';
    }).join('');
    $('#po-facts').innerHTML = [['Owner', u.seller], ['Purchased', u.when], ['Delivery', u.eta]].map(function (f) {
      return '<li style="display:flex;justify-content:space-between;padding:11px 0;border-bottom:.5px solid var(--g200);font-size:var(--f11)">' +
        '<span style="color:var(--g500)">' + f[0] + '</span><b style="font-weight:400">' + f[1] + '</b></li>';
    }).join('');
    go('popdp');
  }

  function paintPOList(filter) {
    var rows = PO.filter(function (u) { return !filter.length || filter.indexOf(u.tier) > -1; });
    $('#pogrid').innerHTML = rows.map(function (u) {
      var p = itemOf(u.of), t = TIERS[u.tier];
      return '<button class="tile" data-po="' + PO.indexOf(u) + '">' +
        '<span class="tile__m"><img src="assets/img/' + u.img + '.webp" alt="">' +
        '<span class="tile__flag">' + t.code + '</span></span>' +
        '<span class="tile__i"><span class="tile__n">' + p.name + '</span>' +
        '<span class="tile__p">' + money(u.price) + ' <span class="u-str">' + money(p.price) + '</span></span></span></button>';
    }).join('') || '<p class="u-met" style="padding:16px">Nothing in that condition right now.</p>';
  }

  /* ---------------------------------------------------------------- bag */
  function addToBag(it) { bag.push(it); paintBagCount(); }
  function paintBagCount() {
    $$('[data-bagc]').forEach(function (e) { e.textContent = String(bag.length); });
  }
  function paintBag() {
    if (!bag.length) {
      $('#bagitems').innerHTML = '<div style="padding:40px 0;text-align:center"><p class="u-lab">YOUR BAG IS EMPTY</p></div>';
      $('#bagsum').hidden = true; $('#bagcta').className = 'btn btn--off'; return;
    }
    $('#bagsum').hidden = false; $('#bagcta').className = 'btn btn--pri';
    $('#bagitems').innerHTML = bag.map(function (it, i) {
      return '<div class="bagrow"><div class="bagrow__m"><img src="assets/img/' + it.img + '.webp" alt=""></div>' +
        '<div>' + (it.pre ? '<p class="u-eye">ZARA PRE-OWNED</p>' : '') +
        '<p style="font-size:var(--f11);margin:4px 0 3px">' + it.name + '</p>' +
        '<p class="u-met">Size ' + it.size + (it.tier ? ' · ' + it.tier : '') + '</p>' +
        (it.eta ? '<p class="u-met">Arrives ' + it.eta + '</p>' : '') +
        '<button class="link" data-rm="' + i + '" style="margin-top:6px">Remove</button></div>' +
        '<div style="font-size:var(--f11)">' + money(it.price) + '</div></div>';
    }).join('');
    var sub = bag.reduce(function (a, b) { return a + b.price; }, 0);
    var anyPre = bag.some(function (b) { return b.pre; });
    var fee = anyPre ? 1 + sub * 0.05 : 0, ship = 4.95;
    $('[data-sub]').textContent = money(sub);
    $('[data-fee]').textContent = money(fee);
    $('[data-ship]').textContent = money(ship);
    $('[data-total]').textContent = money(sub + fee + ship);
    $('[data-feerow]').hidden = !anyPre;
    $('[data-feenote]').hidden = !anyPre;
  }

  /* --------------------------------------------------------------- init */
  document.addEventListener('DOMContentLoaded', function () {
    paintPLP(); paintSizes(); paintBagCount(); paintPOList([]);
    var chosen = null, filter = [];

    document.addEventListener('click', function (e) {
      var t = e.target;
      var hit = function (sel) { return t.closest && t.closest(sel); };

      if (hit('[data-back]')) return back();
      if (hit('[data-close]') || t.id === 'sheetbg') return closeSheets();
      if (hit('[data-restart]')) { bag = []; hist = []; paintBagCount(); return go('home', false); }

      var g = hit('[data-go]'); if (g) { var v = g.getAttribute('data-go'); if (v === 'bag') paintBag(); return go(v); }
      var op = hit('[data-open]'); if (op) return openProduct(op.getAttribute('data-open'));

      var sh = hit('[data-sheet]');
      if (sh) { var n = sh.getAttribute('data-sheet'); if (n === 'po') paintUnits(); else { chosen = null; paintSizes(); } return openSheet(n); }

      var sr = hit('.srow[data-size]');
      if (sr) { $$('.srow').forEach(function (r) { r.classList.remove('is-sel'); });
        sr.classList.add('is-sel'); chosen = sr.getAttribute('data-size'); return; }

      if (hit('[data-pojump]')) { paintUnits(); closeSheets(); return setTimeout(function(){ openSheet('po'); }, 180); }

      if (hit('[data-add-new]')) {
        if (!chosen) return toast('Select a size');
        addToBag({ name: product.name + ' - ' + product.col, price: product.price, img: product.img, size: chosen });
        closeSheets(); toast('Added to bag'); paintBag(); return go('bag');
      }

      var un = hit('[data-unit]'); if (un) return openListing(poFor(product.id)[+un.getAttribute('data-unit')]);
      var pl = hit('[data-po]');   if (pl) { var u = PO[+pl.getAttribute('data-po')]; product = itemOf(u.of); return openListing(u); }

      if (hit('[data-add-po]')) {
        var p = itemOf(listing.of);
        addToBag({ name: p.name + ' - ' + p.col, price: listing.price, img: listing.img, size: 'M',
                   tier: TIERS[listing.tier].code, eta: listing.eta, pre: true });
        toast('Added to bag'); paintBag(); return go('bag');
      }

      var rm = hit('[data-rm]'); if (rm) { bag.splice(+rm.getAttribute('data-rm'), 1); paintBagCount(); return paintBag(); }

      var cf = hit('#cond [data-tier]');
      if (cf) { var k = cf.getAttribute('data-tier');
        var i = filter.indexOf(k); if (i > -1) filter.splice(i, 1); else filter.push(k);
        cf.classList.toggle('btn--quiet'); cf.classList.toggle('btn--sec');
        return paintPOList(filter); }

      if (hit('#v-done')) return;
    });

    // confirmation copy depends on what was bought
    var obs = new MutationObserver(function () {
      if ($('#v-done').classList.contains('is-on')) {
        var pre = bag.some(function (b) { return b.pre; });
        $('#done-sub').textContent = pre
          ? 'Order 44 118 902. Arrives ' + (bag.filter(function(b){return b.pre;})[0].eta) + ', shipped from the Zara store that receives and confirms the garment.'
          : 'Order 44 118 902. Arrives 18–20 Nov.';
      }
    });
    obs.observe($('#v-done'), { attributes: true, attributeFilter: ['class'] });
  });
})();
