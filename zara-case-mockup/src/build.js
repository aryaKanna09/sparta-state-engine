'use strict';
/* Static site generator. No dependencies — `node src/build.js`. */
const fs = require('fs');
const path = require('path');
const L = require('./lib/layout');
const buyer = require('./pages/buyer');
const seller = require('./pages/seller');
const meta = require('./pages/meta');

const OUT = path.join(__dirname, '..', 'public');

/* The walkthrough, in presentation order. */
const MANIFEST = [
  { file: 'home.html', group: 'buyer', title: 'Homepage', render: buyer.home,
    blurb: 'zara.com as it is: full-bleed campaign imagery, the wordmark set across the photograph, no prices.',
    note: 'Zara as it is today. One screen, no scroll.' },

  { file: 'plp.html', group: 'buyer', title: 'Category listing', render: buyer.plp,
    blurb: 'Blazers. 2:3 tiles, hover swaps to the second shot, the VIEW 1/2 density control.',
    note: 'She finds the blazer.' },

  { file: 'pdp.html', group: 'buyer', title: 'Product page — size M is gone', render: buyer.pdp,
    blurb: 'The dead end. M is struck through. Today that is where the journey ends.',
    note: 'The moment of highest intent and guaranteed disappointment.' },

  { file: 'pdp-reloop.html', group: 'buyer', title: 'Pre-owned in your size', render: buyer.pdpSheet,
    blurb: 'The intervention: three graded, priced units of this exact product code, in the size that is gone.',
    note: 'The dead end becomes a shelf.' },

  { file: 'reloop-request.html', group: 'buyer', title: 'Asking the owners', render: buyer.reloopRequest,
    blurb: 'If nothing is listed, the click becomes a demand signal sent to the 14 people who own it.',
    note: 'Demand creates the supply.' },

  { file: 'preowned-pdp.html', group: 'buyer', title: 'Pre-owned product page', render: buyer.preownedPdp,
    blurb: "Zara's catalogue packshot beside the owner's own photographs, with the condition report.",
    note: 'Dual imagery is a real Zara Pre-Owned feature.' },

  { file: 'preowned.html', group: 'buyer', title: 'Pre-owned catalogue', render: buyer.preowned,
    blurb: 'The browsable surface, filterable by condition — which Zara cannot do today, because nothing is graded.',
    note: 'Supply keeps working after the trigger that created it.' },

  { file: 'bag.html', group: 'buyer', title: 'Bag and fees', render: buyer.bag,
    blurb: "The real Zara Resell economics: the buyer pays $1.00 + 5%, the seller keeps their full asking price.",
    note: 'Same checkout. Longer delivery window.' },

  { file: 'order.html', group: 'buyer', title: 'Order confirmed', render: buyer.order,
    blurb: 'A sale that did not exist ten minutes ago, on a unit that was already lost.',
    note: 'Nothing was cannibalised.' },

  { file: 'seller-notify.html', group: 'seller', title: 'The notification', render: seller.notify,
    blurb: '"3 people are looking for the blazer you bought in March." The ask goes to the owner.',
    note: 'Matched on the purchase record, not the catalogue.' },

  { file: 'seller-closet.html', group: 'seller', title: 'Purchase history as supply', render: seller.closet,
    blurb: 'My Purchases already exists in the Zara app. Attaching demand to it turns receipts into inventory.',
    note: 'Authentication is free here.' },

  { file: 'seller-capture.html', group: 'seller', title: 'Guided capture', render: seller.capture,
    blurb: 'Four coached photographs — front, back, the Zara tag, and any wear. No listing to write.',
    note: 'Grading is a photography problem first.' },

  { file: 'seller-offer.html', group: 'seller', title: 'Grade and price', render: seller.offer,
    blurb: 'Three tiers, four named checks, a band anchored to observed resale. Approve, decline or appeal.',
    note: 'Tier names are our assumption — label them.' },

  { file: 'seller-appeal.html', group: 'seller', title: 'Appeal', render: seller.appeal,
    blurb: 'An automated grade that cannot be challenged is one sellers stop believing.',
    note: 'The stated tolerance closes the obvious question.' },

  { file: 'seller-dropoff.html', group: 'seller', title: 'Drop-off or ship', render: seller.dropoff,
    blurb: 'Store returns run in reverse, then ship-from-store. No new distribution centre, no new carrier.',
    note: 'The logistics question, answered on screen.' },

  { file: 'seller-status.html', group: 'seller', title: 'Status and payout', render: seller.status,
    blurb: 'Five states, no messages to answer, and the seller keeps 100% of the asking price.',
    note: 'Four photographs and one decision.' },

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
      wt: {
        step: p.step,
        total,
        prev: i > 0 ? `/${steps[i - 1].file}` : null,
        next: i < total - 1 ? `/${steps[i + 1].file}` : null,
        note: p.note || '',
      },
    });
    fs.writeFileSync(path.join(OUT, p.file), html);
  });

  // contents page
  fs.writeFileSync(
    path.join(OUT, 'index.html'),
    L.doc({ title: 'ZARA RELOOP — Team Fireworks', body: meta.index(steps) })
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
