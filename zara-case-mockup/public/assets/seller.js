/* =========================================================================
   ZARA RELOOP — seller journey, as a working app.
   Notification -> closet -> guided capture (real camera) -> grade -> offer
   -> appeal -> drop-off -> payout. Every step is tappable.
   Camera capture and the capture coaching are REAL. The grade is simulated.
   Presenter keys: 1/2/3 lock the tier, Space fires the shutter.
   ========================================================================= */
(function () {
  'use strict';

  var $ = function (s, r) { return (r || document).querySelector(s); };
  var $$ = function (s, r) { return [].slice.call((r || document).querySelectorAll(s)); };
  var money = function (n) { return '$' + n.toFixed(2); };

  var RETAIL = 109.0;
  var TIERS = {
    e: { code: 'EXCELLENT', pct: 0.75, blurb: 'Worn a handful of times. No visible wear.' },
    g: { code: 'GOOD',      pct: 0.58, blurb: 'Light, even wear. Shape and colour intact.' },
    f: { code: 'FAIR',      pct: 0.42, blurb: 'Visible wear, photographed in detail.' }
  };
  var STEPS = [
    { label: 'FRONT, FLAT, DAYLIGHT', coach: 'Lay the blazer flat near a window. Avoid overhead light.' },
    { label: 'BACK',                  coach: 'Same distance. Keep the whole garment inside the frame.' },
    { label: 'THE ZARA TAG',          coach: 'Hold steady until the product code is sharp.' },
    { label: 'ANY WEAR, UP CLOSE',    coach: 'Cuffs, seams and lining. Flaws raise trust, not risk.' }
  ];
  var CHECKS = [
    ['Pilling & surface', 'No pilling detected'],
    ['Seams & structure', 'Intact, shoulder line true'],
    ['Staining & marks', 'None detected'],
    ['Hardware & fastenings', 'All buttons present']
  ];

  var view = 'lock', idx = 0, shots = [], forced = null, tier = 'e';
  var video, cvs, cx, stream = null, live = false, raf = null, lastA = null;
  var HISTORY = [];

  /* ------------------------------------------------------------- routing */
  function go(name, push) {
    if (push !== false && view !== name) HISTORY.push(view);
    view = name;
    $$('.sv').forEach(function (v) { v.hidden = v.id !== 'v-' + name; });
    var back = $('#back');
    if (back) back.style.visibility = HISTORY.length ? 'visible' : 'hidden';
    if (name !== 'capture') stopLoop();
    window.scrollTo({ top: 0, behavior: 'smooth' });
    document.body.setAttribute('data-view', name);
  }
  function back() {
    if (!HISTORY.length) return;
    go(HISTORY.pop(), false);
  }

  /* ------------------------------------------------------------ analysis */
  var W = 128, H = 192;
  function analyse() {
    if (!live || !video.videoWidth) return null;
    cx.drawImage(video, 0, 0, W, H);
    var d;
    try { d = cx.getImageData(0, 0, W, H).data; } catch (e) { return null; }
    var g = new Float32Array(W * H), sum = 0;
    for (var i = 0, p = 0; i < d.length; i += 4, p++) {
      g[p] = 0.2126 * d[i] + 0.7152 * d[i + 1] + 0.0722 * d[i + 2]; sum += g[p];
    }
    var mean = sum / (W * H);
    var lsum = 0, lsq = 0, n = 0;
    for (var y = 1; y < H - 1; y++) for (var x = 1; x < W - 1; x++) {
      var o = y * W + x;
      var l = 4 * g[o] - g[o - 1] - g[o + 1] - g[o - W] - g[o + W];
      lsum += l; lsq += l * l; n++;
    }
    var sharp = Math.sqrt(lsq / n - (lsum / n) * (lsum / n));
    var inE = 0, outE = 0, x0 = W * .12, x1 = W * .88, y0 = H * .14, y1 = H * .86;
    for (var yy = 1; yy < H - 1; yy++) for (var xx = 1; xx < W - 1; xx++) {
      var oo = yy * W + xx;
      var e = Math.abs(g[oo] - g[oo + 1]) + Math.abs(g[oo] - g[oo + W]);
      if (xx > x0 && xx < x1 && yy > y0 && yy < y1) inE += e; else outE += e;
    }
    var framing = inE / (inE + outE + 1);
    return { light: mean, sharp: sharp, framing: framing,
      okLight: mean > 55 && mean < 215, okSharp: sharp > 7.5, okFrame: framing > 0.58 };
  }

  function chip(id, ok, text) {
    var el = $('#chip-' + id); if (!el) return;
    el.textContent = text; el.className = 'gchip' + (ok ? ' is-ok' : '');
  }

  function loop() {
    if (view !== 'capture') return;
    var a = analyse();
    if (a) {
      lastA = a;
      chip('light', a.okLight, a.light < 55 ? 'TOO DARK' : a.light > 215 ? 'TOO BRIGHT' : 'LIGHT OK');
      chip('sharp', a.okSharp, a.okSharp ? 'SHARP' : 'HOLD STEADY');
      chip('frame', a.okFrame, a.okFrame ? 'FRAMED' : 'MOVE CLOSER');
      $('#shutter').classList.toggle('is-ready', a.okLight && a.okSharp && a.okFrame);
      $('#readout').textContent = 'exposure ' + Math.round(a.light) +
        '  ·  sharpness ' + a.sharp.toFixed(1) + '  ·  framing ' + Math.round(a.framing * 100) + '%';
    }
    raf = requestAnimationFrame(loop);
  }
  function stopLoop() { if (raf) { cancelAnimationFrame(raf); raf = null; } }

  /* -------------------------------------------------------------- camera */
  function startCamera() {
    $('#camnote').textContent = 'Requesting the camera…';
    if (!navigator.mediaDevices || !navigator.mediaDevices.getUserMedia) return sample('No camera API in this browser.');
    navigator.mediaDevices.getUserMedia({ video: { facingMode: { ideal: 'environment' } }, audio: false })
      .then(onStream)
      .catch(function () {
        navigator.mediaDevices.getUserMedia({ video: true, audio: false }).then(onStream)
          .catch(function (e) {
            sample(e && e.name === 'NotAllowedError' ? 'Camera permission declined.' : 'No camera available.');
          });
      });
  }
  function onStream(s) {
    stream = s; video.srcObject = s; video.play();
    video.onloadedmetadata = function () {
      live = true; $('#still').style.display = 'none'; video.style.display = 'block';
      paintStep(); go('capture'); loop();
    };
  }
  /* The flow must never dead-end, so it completes on a bundled photograph. */
  function sample(why) {
    live = false;
    video.style.display = 'none';
    $('#still').style.display = 'block';
    $('#still').src = 'assets/img/capture.webp';
    chip('light', true, 'LIGHT OK'); chip('sharp', true, 'SHARP'); chip('frame', true, 'FRAMED');
    $('#shutter').classList.add('is-ready');
    $('#readout').textContent = why ? why + ' Using the sample garment.' : 'sample garment';
    lastA = { okLight: true, okSharp: true, okFrame: true };
    paintStep(); go('capture');
  }

  /* ------------------------------------------------------------- capture */
  function paintStep() {
    var s = STEPS[idx];
    $('#clabel').textContent = s.label;
    $('#ccoach').textContent = s.coach;
    $('#cstep').textContent = String(idx + 1).padStart(2, '0') + ' / 04';
    $$('#cdots i').forEach(function (d, i) { d.className = i <= idx ? 'on' : ''; });
  }

  function shoot() {
    if (view !== 'capture') return;
    var c = document.createElement('canvas');
    c.width = 300; c.height = 450;
    var g = c.getContext('2d');
    try { g.drawImage(live ? video : $('#still'), 0, 0, c.width, c.height); } catch (e) {}
    shots.push({ data: c.toDataURL('image/jpeg', .72), a: lastA });

    var t = document.createElement('div');
    t.className = 'gthumb';
    t.innerHTML = '<img src="' + shots[shots.length - 1].data + '" alt=""><span>' + shots.length + '</span>';
    $('#thumbs').appendChild(t);

    idx++;
    if (idx >= STEPS.length) return grade();
    paintStep();
  }

  /* -------------------------------------------------------------- grading */
  function grade() {
    stopLoop();
    go('grading');
    var q = 0, c = 0;
    shots.forEach(function (s) {
      if (!s.a) return; c++;
      q += (s.a.okLight ? 1 : 0) + (s.a.okSharp ? 1 : 0) + (s.a.okFrame ? 1 : 0);
    });
    var quality = c ? q / (c * 3) : 1;
    var rows = $$('#ticks li');
    rows.forEach(function (r) { r.classList.remove('done'); });
    var i = 0;
    var iv = setInterval(function () {
      if (i < rows.length) rows[i].classList.add('done');
      if (++i > rows.length) { clearInterval(iv); showOffer(forced || (quality > .8 ? 'e' : quality > .5 ? 'g' : 'f'), quality); }
    }, 480);
  }

  function showOffer(key, quality) {
    tier = key;
    var t = TIERS[key], price = Math.round(RETAIL * t.pct), lo = 38, hi = 92;
    var pos = Math.max(4, Math.min(96, ((price - lo) / (hi - lo)) * 100));
    $('#otier').innerHTML = '<span class="tierdot tierdot--' + key + '"></span>' + t.code;
    $('#oamt').textContent = money(price);
    $('#oblurb').textContent = t.blurb;
    $('#ofill').style.width = pos + '%';
    $('#opin').style.left = pos + '%';
    $('#oband').textContent = money(lo) + ' lowest · ' + money(hi) + ' highest, last 90 days';
    $('#oqual').textContent = 'Capture quality ' + Math.round((quality == null ? 1 : quality) * 100) + '%' +
      (forced ? ' · tier set by presenter' : '');
    $('#oaccept').textContent = 'ACCEPT ' + money(price);
    $('#payamt').textContent = money(price);
    $('#statusprice').textContent = money(price);
    $('#statustier').textContent = t.code;
    var g = $('#oshots'); g.innerHTML = '';
    shots.forEach(function (s) { var im = new Image(); im.src = s.data; g.appendChild(im); });
    go('offer');
  }

  function reset() {
    idx = 0; shots = []; forced = null; lastA = null;
    $('#thumbs').innerHTML = '';
    $('#forcedtag').hidden = true;
    HISTORY = [];
    go('lock', false);
  }

  /* ----------------------------------------------------------------- init */
  document.addEventListener('DOMContentLoaded', function () {
    video = $('#cam');
    cvs = document.createElement('canvas'); cvs.width = W; cvs.height = H;
    cx = cvs.getContext('2d', { willReadFrequently: true });

    // every control in the app
    document.addEventListener('click', function (e) {
      var el = e.target.closest && e.target.closest('[data-go],[data-act]');
      if (!el) return;
      e.preventDefault();
      var act = el.getAttribute('data-act');
      if (act === 'camera') return startCamera();
      if (act === 'sample') return sample('');
      if (act === 'shoot') return shoot();
      if (act === 'reset') return reset();
      if (act === 'back') return back();
      if (act === 'appeal-ok') {
        // an upheld appeal reissues the offer one tier higher
        var order = ['f', 'g', 'e'], i = order.indexOf(tier);
        return showOffer(order[Math.min(order.length - 1, i + 1)], 1);
      }
      var to = el.getAttribute('data-go');
      if (to) go(to);
    });

    // choice rows behave like radios
    document.addEventListener('click', function (e) {
      var c = e.target.closest && e.target.closest('.choice,.opt');
      if (!c) return;
      $$('.' + (c.classList.contains('opt') ? 'opt' : 'choice'), c.parentNode)
        .forEach(function (x) { x.classList.remove('is-on'); });
      c.classList.add('is-on');
    });

    document.addEventListener('keydown', function (e) {
      if (e.target && /INPUT|TEXTAREA/.test(e.target.tagName)) return;
      if ('123'.indexOf(e.key) > -1) {
        forced = { 1: 'e', 2: 'g', 3: 'f' }[e.key];
        var b = $('#forcedtag'); b.hidden = false;
        b.textContent = 'TIER LOCKED: ' + TIERS[forced].code;
      }
      if (e.key === ' ' && view === 'capture') { e.preventDefault(); shoot(); }
    });

    window.addEventListener('pagehide', function () {
      if (stream) stream.getTracks().forEach(function (t) { t.stop(); });
    });

    go('lock', false);
  });
})();
