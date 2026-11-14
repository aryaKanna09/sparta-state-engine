'use strict';
/* Static site generator. No dependencies — `node src/build.js`. */
const fs = require('fs');
const path = require('path');
const L = require('./lib/layout');
const buyer = require('./pages/buyer');
const meta = require('./pages/meta');
const sellerApp = require('./pages/sellerapp');

const OUT = path.join(__dirname, '..', 'public');

/* The walkthrough, in presentation order. */
const MANIFEST = [
  { file: 'home.html', chrome: 'zara.com/us/', group: 'buyer', title: 'Homepage', render: buyer.home,
    blurb: 'zara.com as it is: full-bleed campaign imagery, the wordmark set across the photograph, no prices.',
    note: 'Zara as it is today. One screen, no scroll.' },

  { file: 'plp.html', chrome: 'zara.com/us/en/woman-blazers-l1055.html', group: 'buyer', title: 'Category listing', render: buyer.plp,
    blurb: 'Blazers. 2:3 tiles, hover swaps to the second shot, the VIEW 1/2 density control.',
    note: 'She finds the blazer.' },

  { file: 'pdp.html', chrome: 'zara.com/us/en/textured-tweed-blazer-p02757397.html', group: 'buyer', title: 'Product page — size M is gone', render: buyer.pdp,
    blurb: 'The dead end. M is struck through. Today that is where the journey ends.',
    note: 'The moment of highest intent and guaranteed disappointment.' },

  { file: 'pdp-reloop.html', chrome: 'zara.com/us/en/textured-tweed-blazer-p02757397.html?preowned=M', group: 'buyer', title: 'Pre-owned in your size', render: buyer.pdpSheet,
    blurb: 'The intervention: three graded, priced units of this exact product code, in the size that is gone.',
    note: 'The dead end becomes a shelf.' },

  { file: 'reloop-request.html', chrome: 'zara.com/us/en/preowned/request-placed', group: 'buyer', title: 'Asking the owners', render: buyer.reloopRequest,
    blurb: 'If nothing is listed, the click becomes a demand signal sent to the 14 people who own it.',
    note: 'Demand creates the supply.' },

  { file: 'preowned-pdp.html', chrome: 'zara.com/us/en/preowned/textured-tweed-blazer-p02757397.html', group: 'buyer', title: 'Pre-owned product page', render: buyer.preownedPdp,
    blurb: "Zara's catalogue packshot beside the owner's own photographs, with the condition report.",
    note: 'Dual imagery is a real Zara Pre-Owned feature.' },

  { file: 'preowned.html', chrome: 'zara.com/us/en/preowned-woman-mkt7630.html', group: 'buyer', title: 'Pre-owned catalogue', render: buyer.preowned,
    blurb: 'The browsable surface, filterable by condition — which Zara cannot do today, because nothing is graded.',
    note: 'Supply keeps working after the trigger that created it.' },

  { file: 'bag.html', chrome: 'zara.com/us/en/shop/cart', group: 'buyer', title: 'Bag and fees', render: buyer.bag,
    blurb: "The real Zara Resell economics: the buyer pays $1.00 + 5%, the seller keeps their full asking price.",
    note: 'Same checkout. Longer delivery window.' },

  { file: 'order.html', chrome: 'zara.com/us/en/order/confirmation', group: 'buyer', title: 'Order confirmed', render: buyer.order,
    blurb: 'A sale that did not exist ten minutes ago, on a unit that was already lost.',
    note: 'Nothing was cannibalised.' },

  { file: 'seller.html', group: 'seller', title: 'The seller journey', render: sellerApp.sellerApp,
    script: 'assets/seller.js',
    blurb: 'Tap it through: notification, your closet, four coached photographs, grade and price, drop-off, payout.',
    note: 'Tap the notification to start. 1/2/3 locks the tier.' },

  { file: 'systems.html', group: 'argument', title: 'Six systems, four already run', render: meta.systems,
    blurb: 'The novelty claim made falsifiable, plus the cannibalisation and catalogue answers.',
    note: 'The line that separates this from a resale app.' },
];

function build() {
  if (!fs.existsSync(OUT)) fs.mkdirSync(OUT, { recursive: true });

  const steps = MANIFEST.map((p, i) => ({ ...p, step: i + 1 }));
  const total = steps.length;

  steps.forEach((p, i) => {
    const html = L.doc({
      title: `${p.title} — ZARA`,
      body: p.render(),
      chrome: p.chrome || null,
      script: p.script || '',
      wt: {
        step: p.step,
        total,
        prev: i > 0 ? `${steps[i - 1].file}` : null,
        next: i < total - 1 ? `${steps[i + 1].file}` : null,
        note: p.note || '',
      },
    });
    fs.writeFileSync(path.join(OUT, p.file), html);
  });

  // The iOS apps are the deliverable, so index.html is their launcher
  // (written by hand as app-index.html). The old website walkthrough keeps
  // its contents page at web-index.html.
  fs.writeFileSync(
    path.join(OUT, 'web-index.html'),
    L.doc({ title: 'ZARA RELOOP — website walkthrough', body: meta.index(steps) })
  );

  // favicon: the wordmark Z
  fs.writeFileSync(
    path.join(OUT, 'assets', 'favicon.svg'),
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32"><rect width="32" height="32" fill="#000"/><text x="16" y="23" font-family="Didot,'Bodoni Moda',serif" font-size="20" fill="#fff" text-anchor="middle">Z</text></svg>`
  );

  console.log(`built ${steps.length + 1} pages -> ${OUT}`);
  steps.forEach((p) => console.log(`  ${String(p.step).padStart(2, '0')}  ${p.file}`));
}

build();
module.exports = { MANIFEST };
