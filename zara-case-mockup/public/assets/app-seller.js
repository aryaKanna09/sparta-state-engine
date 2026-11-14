/* ZARA iOS — seller. The prompt is a price, not a demand claim.
   Camera capture and capture coaching are REAL; the grade is simulated.
   Presenter keys: 1/2/3 lock the tier, Space fires the shutter. */
(function () {
  'use strict';
  var $ = function (s, r) { return (r || document).querySelector(s); };
  var $$ = function (s, r) { return [].slice.call((r || document).querySelectorAll(s)); };
  var money = function (n) { return '$' + n.toFixed(2); };

  var RETAIL = 109;
  var TIERS = {
    e: { code: 'LIKE NEW', pct: .75, blurb: 'Worn a handful of times. No visible wear.' },
    g: { code: 'GOOD',      pct: .58, blurb: 'Light, even wear. Shape and colour intact.' },
    f: { code: 'FAIR',      pct: .42, blurb: 'Visible wear, photographed in detail.' }
  };
  var STEPS = [
    ['FRONT, FLAT, DAYLIGHT', 'Lay the blazer flat near a window. Avoid overhead light.'],
    ['BACK', 'Same distance. Keep the whole garment inside the frame.'],
    ['THE ZARA TAG', 'Hold steady until the product code is sharp.'],
    ['ANY WEAR, UP CLOSE', 'Cuffs, seams and lining. Flaws raise trust, not risk.']
  ];

  var view = 'lock', hist = [], idx = 0, shots = [], forced = null, tier = 'e';
  var fulfil = 'store';
  var video, cvs, cx, stream = null, live = false, raf = null, lastA = null;

  /* ------------------------------------------------------------ routing */
  function go(v, push) {
    if (push !== false && v !== view) hist.push(view);
    view = v;
    $$('.view').forEach(function (s) { s.classList.toggle('is-on', s.id === 'v-' + v); });
    if (v !== 'capture') stopLoop();
    var sc = $('#v-' + v + ' .scroll'); if (sc) sc.scrollTop = 0;
  }
  function back() { var p = hist.pop(); if (p) go(p, false); }
  function toast(m) {
    var t = $('#toast'); t.textContent = m; t.classList.add('is-on');
    setTimeout(function () { t.classList.remove('is-on'); }, 1900);
  }

  /* ----------------------------------------------------------- analysis */
  var W = 128, H = 192;
  function analyse() {
    if (!live || !video.videoWidth) return null;
    cx.drawImage(video, 0, 0, W, H);
    var d; try { d = cx.getImageData(0, 0, W, H).data; } catch (e) { return null; }
    var g = new Float32Array(W * H), sum = 0;
    for (var i = 0, p = 0; i < d.length; i += 4, p++) {
      g[p] = .2126 * d[i] + .7152 * d[i + 1] + .0722 * d[i + 2]; sum += g[p];
    }
    var mean = sum / (W * H), ls = 0, lq = 0, n = 0;
    for (var y = 1; y < H - 1; y++) for (var x = 1; x < W - 1; x++) {
      var o = y * W + x, l = 4 * g[o] - g[o - 1] - g[o + 1] - g[o - W] - g[o + W];
      ls += l; lq += l * l; n++;
    }
    var sharp = Math.sqrt(lq / n - (ls / n) * (ls / n));
    var inE = 0, outE = 0, x0 = W * .12, x1 = W * .88, y0 = H * .14, y1 = H * .86;
    for (var yy = 1; yy < H - 1; yy++) for (var xx = 1; xx < W - 1; xx++) {
      var oo = yy * W + xx, e = Math.abs(g[oo] - g[oo + 1]) + Math.abs(g[oo] - g[oo + W]);
      if (xx > x0 && xx < x1 && yy > y0 && yy < y1) inE += e; else outE += e;
    }
    var fr = inE / (inE + outE + 1);
    return { light: mean, sharp: sharp, framing: fr,
      okLight: mean > 55 && mean < 215, okSharp: sharp > 7.5, okFrame: fr > .58 };
  }
  function chip(id, ok, txt) { var e = $('#chip-' + id); if (e) { e.textContent = txt; e.className = 'chip' + (ok ? ' ok' : ''); } }
  function loop() {
    if (view !== 'capture') return;
    var a = analyse();
    if (a) {
      lastA = a;
      chip('light', a.okLight, a.light < 55 ? 'TOO DARK' : a.light > 215 ? 'TOO BRIGHT' : 'LIGHT OK');
      chip('sharp', a.okSharp, a.okSharp ? 'SHARP' : 'HOLD STEADY');
      chip('frame', a.okFrame, a.okFrame ? 'FRAMED' : 'MOVE CLOSER');
      $('#shutter').classList.toggle('ready', a.okLight && a.okSharp && a.okFrame);
      $('#readout').textContent = 'exposure ' + Math.round(a.light) + ' · sharpness ' +
        a.sharp.toFixed(1) + ' · framing ' + Math.round(a.framing * 100) + '%';
    }
    raf = requestAnimationFrame(loop);
  }
  function stopLoop() { if (raf) { cancelAnimationFrame(raf); raf = null; } }

  /* ------------------------------------------------------------- camera */
  function startCamera() {
    if (!navigator.mediaDevices || !navigator.mediaDevices.getUserMedia) return sample('No camera API.');
    navigator.mediaDevices.getUserMedia({ video: { facingMode: { ideal: 'environment' } }, audio: false })
      .then(onStream).catch(function () {
        navigator.mediaDevices.getUserMedia({ video: true, audio: false }).then(onStream)
          .catch(function (e) { sample(e && e.name === 'NotAllowedError' ? 'Camera declined.' : 'No camera.'); });
      });
  }
  function onStream(s) {
    stream = s; video.srcObject = s; video.play();
    video.onloadedmetadata = function () {
      live = true; $('#still').style.display = 'none'; video.style.display = 'block';
      paintStep(); go('capture'); loop();
    };
  }
  function sample(why) {
    live = false; video.style.display = 'none';
    $('#still').style.display = 'block'; $('#still').src = 'assets/img/capture.webp';
    chip('light', true, 'LIGHT OK'); chip('sharp', true, 'SHARP'); chip('frame', true, 'FRAMED');
    $('#shutter').classList.add('ready');
    $('#readout').textContent = why ? why + ' Using the sample garment.' : 'sample garment';
    lastA = { okLight: true, okSharp: true, okFrame: true };
    paintStep(); go('capture');
  }

  /* ------------------------------------------------------------ capture */
  function paintStep() {
    $('#clabel').textContent = STEPS[idx][0];
    $('#ccoach').textContent = STEPS[idx][1];
    $$('#cdots i').forEach(function (d, i) { d.className = i <= idx ? 'on' : ''; });
  }
  function shoot() {
    if (view !== 'capture') return;
    var c = document.createElement('canvas'); c.width = 300; c.height = 450;
    try { c.getContext('2d').drawImage(live ? video : $('#still'), 0, 0, c.width, c.height); } catch (e) {}
    shots.push({ data: c.toDataURL('image/jpeg', .72), a: lastA });
    var t = document.createElement('div'); t.className = 'thumb';
    t.innerHTML = '<img src="' + shots[shots.length - 1].data + '" alt=""><span>' + shots.length + '</span>';
    $('#thumbs').appendChild(t);
    idx++;
    if (idx >= STEPS.length) return grade();
    paintStep();
  }

  /* ------------------------------------------------------------- grading */
  function grade() {
    stopLoop(); go('grading');
    var q = 0, c = 0;
    shots.forEach(function (s) {
      if (!s.a) return; c++;
      q += (s.a.okLight ? 1 : 0) + (s.a.okSharp ? 1 : 0) + (s.a.okFrame ? 1 : 0);
    });
    var quality = c ? q / (c * 3) : 1;
    var rows = $$('#ticks li'); rows.forEach(function (r) { r.classList.remove('done'); });
    var i = 0, iv = setInterval(function () {
      if (i < rows.length) rows[i].classList.add('done');
      if (++i > rows.length) { clearInterval(iv); offer(forced || (quality > .8 ? 'e' : quality > .5 ? 'g' : 'f'), quality); }
    }, 460);
  }

  function offer(key, quality) {
    tier = key;
    var t = TIERS[key], price = Math.round(RETAIL * t.pct), lo = 38, hi = 92;
    var pos = Math.max(4, Math.min(96, ((price - lo) / (hi - lo)) * 100));
    $('#otier-dot').className = 'dot dot--' + key;
    $('#otier').textContent = t.code;
    $('#oamt').textContent = money(price);
    $('#oblurb').textContent = t.blurb;
    $('#ofill').style.width = pos + '%';
    $('#opin').style.left = pos + '%';
    $('#oband').textContent = money(lo) + ' lowest · ' + money(hi) + ' highest, last 90 days';
    $('#oqual').textContent = 'Capture quality ' + Math.round((quality == null ? 1 : quality) * 100) + '%' +
      (forced ? ' · tier set by presenter' : '');
    $('#oaccept').textContent = 'ACCEPT ' + money(price);
    ['#listed-price', '#next-price'].forEach(function (s) { $(s).textContent = money(price); });
    ['#listed-tier', '#next-tier'].forEach(function (s) { $(s).textContent = t.code; });
    $('#payamt').textContent = money(price);
    var g = $('#oshots'); g.innerHTML = '';
    shots.forEach(function (s) { var im = new Image(); im.src = s.data; g.appendChild(im); });
    go('offer');
  }

  /* --------------------------------------------- fulfilment preference */
  function setFulfil(kind) {
    fulfil = kind;
    var store = kind === 'store';
    $('#listed-pref').textContent = store ? 'drop it at Zara SoHo' : 'post it with the prepaid label';
    $('#push-next').textContent  = store ? 'Drop it at Zara SoHo' : 'Post it with your prepaid label';
    $('#next-steps').innerHTML = store ? [
      '<p class="u-eye">WHAT TO DO</p>',
      '<ol class="track" style="margin-top:8px">',
      '<li class="track__n done"><div class="track__d"></div><div><div class="track__t">Take it to Zara SoHo</div>',
      '<div class="track__s">580 Broadway · open until 9pm · no packaging needed</div></div></li>',
      '<li class="track__n"><div class="track__d"></div><div><div class="track__t">Hand it to any staff member</div>',
      '<div class="track__s">They scan the code in this screen. Nothing to print.</div></div></li>',
      '<li class="track__n"><div class="track__d"></div><div><div class="track__t">They confirm the grade and send it</div>',
      '<div class="track__s">The store ships it to the buyer on ship-from-store.</div></div></li></ol>',
      '<div style="background:#f4f3f1;padding:13px;margin-top:14px"><p class="u-eye">WHY THIS MATTERS</p>',
      '<p class="u-met" style="margin:5px 0 0">Today you would package this yourself and post it within three days,',
      ' and Zara would never touch it — which is why it cannot guarantee condition and why all sales are final.',
      ' A staff member confirming the grade is what turns your claim into Zara\'s guarantee.</p></div>'
    ].join('') : [
      '<p class="u-eye">WHAT TO DO</p>',
      '<ol class="track" style="margin-top:8px">',
      '<li class="track__n done"><div class="track__d"></div><div><div class="track__t">Your prepaid label is in your email</div>',
      '<div class="track__s">Nothing to pay. Print it or show the QR.</div></div></li>',
      '<li class="track__n"><div class="track__d"></div><div><div class="track__t">Drop it at any carrier point</div>',
      '<div class="track__s">Within three days, as on Zara Resell today.</div></div></li>',
      '<li class="track__n"><div class="track__d"></div><div><div class="track__t">We confirm the grade on arrival</div>',
      '<div class="track__s">Then it ships to the buyer.</div></div></li></ol>'
    ].join('');
  }

  function reset() {
    idx = 0; shots = []; forced = null; lastA = null; hist = [];
    $('#thumbs').innerHTML = '';
    go('lock', false);
  }

  /* ---------------------------------------------------------------- init */
  document.addEventListener('DOMContentLoaded', function () {
    video = $('#cam');
    cvs = document.createElement('canvas'); cvs.width = W; cvs.height = H;
    cx = cvs.getContext('2d', { willReadFrequently: true });
    setFulfil('store');

    document.addEventListener('click', function (e) {
      var hit = function (s) { return e.target.closest && e.target.closest(s); };
      if (hit('[data-back]')) return back();

      var f = hit('[data-fulfil]');
      if (f) {
        $$('[data-fulfil]').forEach(function (o) { o.classList.remove('is-on'); });
        f.classList.add('is-on');
        return setFulfil(f.getAttribute('data-fulfil'));
      }

      var o = hit('.opt:not([data-fulfil])');
      if (o) { $$('.opt:not([data-fulfil])', o.parentNode).forEach(function (x) { x.classList.remove('is-on'); }); o.classList.add('is-on'); }

      var a = hit('[data-act]');
      if (a) {
        var k = a.getAttribute('data-act');
        if (k === 'camera') return startCamera();
        if (k === 'sample') return sample('');
        if (k === 'shoot')  return shoot();
        if (k === 'restart') return reset();
        if (k === 'appeal-ok') {
          var order = ['f', 'g', 'e'], i = order.indexOf(tier);
          toast('Appeal upheld');
          return offer(order[Math.min(2, i + 1)], 1);
        }
      }
      var g = hit('[data-go]'); if (g) return go(g.getAttribute('data-go'));
    });

    document.addEventListener('keydown', function (ev) {
      if (ev.target && /INPUT|TEXTAREA/.test(ev.target.tagName)) return;
      if ('123'.indexOf(ev.key) > -1) { forced = { 1: 'e', 2: 'g', 3: 'f' }[ev.key]; toast('Tier locked: ' + TIERS[forced].code); }
      if (ev.key === ' ' && view === 'capture') { ev.preventDefault(); shoot(); }
    });

    window.addEventListener('pagehide', function () {
      if (stream) stream.getTracks().forEach(function (t) { t.stop(); });
    });
  });
})();
