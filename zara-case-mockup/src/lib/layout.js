'use strict';
/* Shared chrome for the Zara mockup. Pure string templates, zero deps. */

const BRAND = {
  // The case-comp feature mark. Change here and it changes everywhere.
  feature: 'RELOOP',
  featureLong: 'ZARA RELOOP',
};

const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

/* ---------------------------------------------------------------- document */
function doc({ title, body, cls = '', wt = null, bodyClass = '', chrome = null, script = '' }) {
  const framed = !!chrome;
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>${esc(title)}</title>
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link rel="stylesheet" href="/assets/zara.css">
<link rel="icon" href="/assets/favicon.svg">
</head>
<body class="${bodyClass}${wt ? ' has-wt' : ''}${framed ? ' has-chrome' : ''} ${cls}">
${framed ? `<div class="winwrap"><div class="win">${browserChrome(chrome)}<div class="win__page">${body}</div></div></div>` : body}
${wt ? walkthrough(wt) : ''}
<script src="/assets/app.js" defer></script>
${script ? `<script src="${script}" defer></script>` : ''}
</body>
</html>`;
}

/* A fictional laptop browser window, so the desktop screens read as real
   screenshots rather than a web page inside a web page. */
function browserChrome(url) {
  return `
<div class="win__bar">
  <span class="win__lights"><i></i><i></i><i></i></span>
  <span class="win__nav">&#8249; &#8250;</span>
  <span class="win__url"><svg width="9" height="11" viewBox="0 0 9 11" aria-hidden="true"><path d="M2 4.5V3a2.5 2.5 0 015 0v1.5" fill="none" stroke="currentColor" stroke-width="1"/><rect x="1" y="4.5" width="7" height="5.5" rx="1" fill="currentColor"/></svg>${esc(url)}</span>
  <span class="win__tools">&#8635;&nbsp;&nbsp;&#8862;</span>
</div>`;
}

/* ------------------------------------------------------------ site header */
/* Real zara.com top-level tabs, verbatim and in order. Note ZARA HOME (not
   "HOME"), and that MASSIMO DUTTI, PRE-OWNED and TRAVEL MODE are all tabs. */
const TABS = ['WOMAN', 'MAN', 'KIDS', 'ZARA HOME', 'BEAUTY', 'MASSIMO DUTTI', 'PRE-OWNED', 'TRAVEL MODE'];

function header({ nav = 'WOMAN', bordered = false, bagCount = 0, logo = true } = {}) {
  const href = (t) => (t === 'PRE-OWNED' ? '/preowned.html' : '/plp.html');
  return `
<header class="site-header${bordered ? ' site-header--bordered' : ''}">
  <div class="hdr-left">
    <button class="hdr-menu" aria-label="Menu"><span></span><span></span><span></span></button>
    ${logo ? '<a class="hdr-logo" href="/home.html"><span class="wordmark">ZARA</span></a>' : ''}
    <nav class="hdr-nav">
      ${TABS.map((t) => `<a href="${href(t)}"${t === nav ? ' aria-current="page"' : ''}>${t}</a>`).join('\n      ')}
    </nav>
  </div>
  <div class="hdr-right">
    <div class="hdr-search"><input type="text" placeholder="Search" aria-label="Search"></div>
    <a href="/bag.html">BAG ${bagCount ? `[${bagCount}]` : '[0]'}</a>
    <a href="/account.html">LOG IN</a>
    <a href="#">HELP</a>
  </div>
</header>`;
}

/* ------------------------------------------------------------ site footer */
function footer() {
  const cols = [
    ['HELP', ['Track your order', 'Returns and exchanges', 'Shipping', 'Payment', 'Gift card', 'Contact us']],
    ['FOLLOW US', ['Newsletter', 'Instagram', 'TikTok', 'Pinterest', 'YouTube', 'Spotify']],
    ['COMPANY', ['About us', 'Offices', 'Work with us', 'Join Life', 'Pre-Owned', 'Press']],
    ['POLICIES', ['Privacy policy', 'Cookie settings', 'Terms of use', 'Purchase conditions', 'Accessibility']],
  ];
  return `
<footer class="site-footer">
  <div class="footer-cols">
    ${cols.map(([h, items]) => `<div>
      <h4>${h}</h4>
      <ul>${items.map((i) => `<li><a href="#">${i}</a></li>`).join('')}</ul>
    </div>`).join('\n    ')}
  </div>
  <div class="footer-base">
    <span>&copy; ZARA 2026</span>
    <a href="#">Cookie settings</a>
    <a href="#">Privacy policy</a>
    <a href="#">Terms of use</a>
    <span>United States / English</span>
  </div>
</footer>`;
}

/* ------------------------------------------------- presenter walkthrough */
/* Not part of the Zara UI. Lets the team click the story in order on stage. */
function walkthrough({ step, total, prev, next, note = '' }) {
  return `
<div class="wt">
  <a href="/">CONTENTS</a>
  <span class="wt__step">STEP ${String(step).padStart(2, '0')} / ${String(total).padStart(2, '0')}</span>
  ${note ? `<span class="wt__side">${note}</span>` : ''}
  <span class="wt__spacer"></span>
  <div class="wt__nav">
    ${prev ? `<a href="${prev}" data-wt-prev>&larr; BACK</a>` : '<span class="wt__step">&larr; BACK</span>'}
    ${next ? `<a href="${next}" data-wt-next>NEXT &rarr;</a>` : '<span class="wt__step">NEXT &rarr;</span>'}
  </div>
</div>`;
}

/* --------------------------------------------------------- phone wrapper */
function phone({ screen, label = '', time = '9:41' }) {
  return `
<div>
  <div class="phone">
    <div class="phone__cap"><span>${time}</span><span class="phone__sig"><i></i><i></i><i></i><i></i></span></div>
    <div class="phone__screen">${screen}</div>
  </div>
  ${label ? `<div class="phone__label">${label}</div>` : ''}
</div>`;
}

/* ------------------------------------------------------------ app chrome */
function appBar({ title, back = true, right = '' }) {
  return `<div class="app-bar">
  <span class="app-bar__back">${back ? '&#8249;' : '&nbsp;'}</span>
  <span class="app-bar__title">${title}</span>
  <span class="app-bar__back">${right || '&nbsp;'}</span>
</div>`;
}

function tabbar(active = '') {
  const t = [
    ['Home', '&#9634;', '/'],
    ['Search', '&#9906;', '/plp.html'],
    ['Pre-Owned', '&#8634;', '/preowned.html'],
    ['Bag', '&#9723;', '/bag.html'],
    ['Account', '&#9711;', '/account.html'],
  ];
  return `<nav class="tabbar">${t
    .map(([n, i, h]) => `<a href="${h}"${n === active ? ' aria-current="page"' : ''}><i>${i}</i>${n}</a>`)
    .join('')}</nav>`;
}

function steps(n, total) {
  return `<div class="steps">${Array.from({ length: total }, (_, i) => `<i class="${i < n ? 'on' : ''}"></i>`).join('')}</div>`;
}

/* ------------------------------------------------------------- utilities */
function note(label, text) {
  return `<div class="note"><b>${label}</b>${text}</div>`;
}

/* IMAGE SLOT.
   Renders a real photograph from /assets/img/<name>.jpg when that file is
   present, and silently falls back to a styled editorial placeholder when it
   is not. Drop Zara's own product shots into public/assets/img/ using the
   documented filenames and every screen picks them up with no code change. */
function pic(name, variant = 'a', alt = '') {
  if (!name) return shot(variant);
  return `<img src="/assets/img/${name}.jpg" alt="${esc(alt)}" loading="lazy"
    onerror="this.outerHTML=document.getElementById('ph-${variant}').innerHTML">`;
}

/* The fallback templates, emitted once per page. */
function phTemplates() {
  return ['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h']
    .map((v) => `<template id="ph-${v}">${shot(v)}</template>`)
    .join('');
}

/* A generated garment "photograph": layered CSS so tiles read as a lookbook
   rather than flat colour. Used whenever a real photo is not present. */
function shot(variant = 'a', opts = {}) {
  const { figure = true, label = '' } = opts;
  return `<div class="ph ph--${variant}">${figure ? figureSvg(variant) : ''}${
    label ? `<span class="ph__cap">${label}</span>` : ''
  }</div>`;
}

/* Minimal editorial silhouette — suggests a garment on a model without
   pretending to be a photo. */
function figureSvg(v) {
  const light = ['d', 'h', 'f'].includes(v);
  const stroke = light ? 'rgba(255,255,255,.26)' : 'rgba(0,0,0,.16)';
  const fill = light ? 'rgba(255,255,255,.1)' : 'rgba(0,0,0,.07)';
  return `<svg class="ph__fig" viewBox="0 0 200 300" preserveAspectRatio="xMidYMax slice" aria-hidden="true"
    style="position:absolute;inset:0;width:100%;height:100%">
    <g fill="${fill}" stroke="${stroke}" stroke-width="1">
      <circle cx="100" cy="62" r="20"/>
      <path d="M78 86 L122 86 L140 112 L132 196 L68 196 L60 112 Z"/>
      <path d="M68 196 L84 300 M132 196 L116 300" fill="none"/>
      <path d="M60 112 L44 168 M140 112 L156 168" fill="none"/>
    </g>
  </svg>`;
}

module.exports = {
  BRAND, esc, doc, header, footer, walkthrough, phone, appBar, tabbar, steps, note,
  shot, figureSvg, pic, phTemplates, browserChrome,
};
