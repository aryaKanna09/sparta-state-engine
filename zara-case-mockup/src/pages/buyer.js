'use strict';
const L = require('../lib/layout');
const D = require('../data');
const { COPY, TIERS, CHECKS, HERO, PRODUCTS, UNITS, money } = D;
const { header, footer, pic, phTemplates, note } = L;

const tierOf = (k) => TIERS.find((t) => t.key === k);

/* ====================================================================== 01
   HOMEPAGE — one screen tall, two full-bleed campaign images side by side,
   wordmark set ACROSS the photograph rather than pinned in the header.
   ====================================================================== */
function home() {
  return `
${header({ nav: 'WOMAN', logo: false })}
<main class="hp">
  <a class="hp__pane" href="plp.html">
    <div class="hp__media">${pic('skirt', 'c', 'Zara campaign')}</div>
    <span class="hp__mark wordmark">ZARA</span>
  </a>
  <a class="hp__pane" href="plp.html">
    <div class="hp__media">${pic('dress', 'b', 'Zara campaign')}</div>
  </a>
  <div class="hp__rail">
    <span class="hp__switch" aria-current="true">COLLECTION</span>
    <span class="hp__switch">EDITORIAL</span>
  </div>
  <div class="hp__arrows"><span>&#8249;</span><span>&#8250;</span></div>
</main>
${phTemplates()}`;
}

/* ====================================================================== 02
   PLP — 2:3 tiles, hover swaps to the second shot, VIEW 1/2 density control.
   ====================================================================== */
function plp() {
  const card = (p) => `
  <a class="card" href="${p.href || '#'}">
    <div class="card__media">
      ${pic(p.img, p.ph, p.name)}
      ${p.alt ? `<div class="alt">${pic(p.alt, p.ph, '')}</div>` : ''}
    </div>
    <div class="card__info">
      <div class="card__name">${p.name}</div>
      <div class="card__price">${money(p.price)}</div>
    </div>
  </a>`;

  return `
${header({ nav: 'WOMAN' })}
<div class="plp-head">
  <span class="plp-title">BLAZERS</span>
  <div class="plp-tools"><button>FILTERS</button></div>
</div>
<main class="grid grid--4">${PRODUCTS.map(card).join('')}</main>
<div class="plp-view"><span>VIEW</span><button aria-current="true">1</button><button>2</button></div>
${footer()}
${phTemplates()}`;
}

/* ====================================================================== 03
   PDP — the pivot. Size M is sold out. Today that is a dead end; here it
   carries the RELOOP affordance.
   ====================================================================== */
function pdp({ sheet = false } = {}) {
  const sizeRow = (z) => {
    if (z.state === 'in') {
      return `<button class="size-row" data-size="${z.s}"><span class="size-row__name">${z.s}</span></button>`;
    }
    if (z.state === 'soon') {
      return `<button class="size-row is-soon"><span class="size-row__name">${z.s}</span>
        <span class="size-row__meta">${COPY.comingSoon}</span></button>`;
    }
    // SOLD OUT — struck through, and this is where we intervene.
    return `<button class="size-row is-out has-reloop" data-open-sheet>
      <span class="size-row__name">${z.s}</span>
      <span class="size-row__reloop">${COPY.reloopCta}</span>
    </button>`;
  };

  return `
${header({ nav: 'WOMAN' })}
<main class="pdp">
  <div class="pdp__gallery">
    ${HERO.gallery.map((g, i) => `<div class="pdp__shot${i === 2 ? ' pdp__shot--tall' : ''}">${pic(g, HERO.ph, HERO.name)}</div>`).join('')}
  </div>

  <aside class="pdp__panel" data-pdp data-name="${HERO.name}" data-colour="${HERO.colour}"
    data-price="${HERO.price}" data-img="${HERO.img}" data-ph="${HERO.ph}">
    <h1 class="pdp__name">${HERO.name} - ${HERO.colour}</h1>
    <p class="pdp__price">${money(HERO.price)}</p>
    <p class="pdp__ref">${HERO.colour} ${HERO.ref}</p>

    <div class="swatches">
      <span class="swatch" aria-checked="true"><i style="background:#8fb4d9"></i></span>
      <span class="swatch"><i style="background:#1e1e1e"></i></span>
      <span class="swatch"><i style="background:#d8cfc2"></i></span>
    </div>

    <div class="sizes">${HERO.sizes.map(sizeRow).join('')}</div>

    <div class="pdp__sublinks">
      <a class="link-u" href="#">${COPY.findYourSize}</a>
      <a class="link-u" href="#">${COPY.checkStore}</a>
    </div>

    <div class="pdp__actions">
      <button class="btn btn--primary btn--block btn--disabled" data-add>${COPY.addToBag}</button>
      <p class="u-meta size-hint" data-size-hint></p>
    </div>

    <p class="pdp__desc">${HERO.desc}</p>

    <div class="acc">
      <button class="acc__row"><span class="u-label">${COPY.composition}</span><span class="acc__ico">+</span></button>
      <div class="acc__body">${HERO.composition}<br>Made in Portugal.</div>
      <button class="acc__row"><span class="u-label">${COPY.shipping}</span><span class="acc__ico">+</span></button>
      <button class="acc__row"><span class="u-label">${COPY.joinlife}</span><span class="acc__ico">+</span></button>
    </div>
  </aside>
</main>
${sheet ? reloopSheet() : ''}
${footer()}
${phTemplates()}`;
}

/* ====================================================================== 04
   THE SHEET — pre-owned units of this exact SKU, in the size that is gone.
   ====================================================================== */
function reloopSheet() {
  const unit = (u) => {
    const t = tierOf(u.tier);
    return `
    <a class="unit" href="preowned-pdp.html">
      <div class="unit__media">${pic(u.img, u.ph, HERO.name)}</div>
      <div>
        <div class="unit__tier"><span class="tierdot tierdot--${u.tier}"></span>${t.code}</div>
        <div class="unit__price">${money(u.price)} <span class="u-strike">${money(HERO.price)}</span></div>
        <div class="unit__note">${u.note}<br>${u.seller} &middot; ${u.eta}</div>
      </div>
    </a>`;
  };

  return `
<div class="sheet-backdrop" data-close-sheet></div>
<aside class="sheet">
  <div class="sheet__head">
    <span class="sheet__title">${COPY.programme} &middot; SIZE M</span>
    <button class="sheet__close" data-close-sheet>&times;</button>
  </div>
  <div class="sheet__body">
    <p class="sheet__lede">${HERO.name} &mdash; ${HERO.colour}<br>
      <span class="u-meta">3 pre-owned in your size, graded and priced by Zara.</span></p>

    ${UNITS.map(unit).join('')}

    <div class="sheet__ask">
      <p class="u-label">NOT THE ONE?</p>
      <p class="u-meta">We can ask the 14 people who bought this blazer in M whether they want to sell it. You will be notified if one is listed.</p>
      <a class="btn btn--outline btn--block" href="reloop-request.html">ASK OWNERS OF THIS ITEM</a>
    </div>
  </div>
</aside>`;
}

function pdpSheet() { return pdp({ sheet: true }); }

/* ====================================================================== 05
   REQUEST PLACED — the demand signal that creates supply.
   ====================================================================== */
function reloopRequest() {
  return `
${header({ nav: 'WOMAN' })}
<main class="narrow">
  <p class="u-eyebrow">${COPY.programme}</p>
  <h1 class="big">We've asked 14 owners.</h1>
  <p class="lede">Everyone who bought the ${HERO.name.toLowerCase()} in ${HERO.colour.toLowerCase()}, size M, has been
  asked whether they want to sell it. You'll be emailed the moment one is listed and graded.</p>

  <div class="facts">
    <div><span class="u-eyebrow">ITEM</span><p>${HERO.name} &mdash; ${HERO.colour}</p></div>
    <div><span class="u-eyebrow">SIZE</span><p>M</p></div>
    <div><span class="u-eyebrow">OWNERS ASKED</span><p>14</p></div>
    <div><span class="u-eyebrow">TYPICAL FIRST LISTING</span><p>Within 48 hours</p></div>
  </div>

  ${note('WHY THIS IS THE WHOLE IDEA', `Today a sold-out size offers at best a 15-day email that expires, and never mentions that
  the same garment may already be sitting on Zara's own resale platform. Here the dead end becomes the trigger: the
  click is a demand signal, and it is routed to the people who are already holding the supply.`)}

  <a class="btn btn--primary" href="preowned.html">BROWSE ${COPY.programme}</a>
</main>
${footer()}
${phTemplates()}`;
}

/* ====================================================================== 06
   PRE-OWNED PDP — Zara's real differentiator: the catalogue packshot shown
   beside the owner's own photographs.
   ====================================================================== */
function preownedPdp() {
  const u = UNITS[0];
  const t = tierOf(u.tier);
  return `
${header({ nav: 'PRE-OWNED' })}
<main class="pdp">
  <div class="pdp__gallery">
    <div class="pdp__shot">
      ${pic(HERO.img, HERO.ph, HERO.name)}
      <span class="shot-tag">AS SOLD NEW</span>
    </div>
    <div class="pdp__shot">
      ${pic(u.img, u.ph, HERO.name)}
      <span class="shot-tag">OWNER'S PHOTO</span>
    </div>
    <div class="pdp__shot pdp__shot--tall">
      ${pic('detail-cuff', 'a', 'Condition detail')}
      <span class="shot-tag">CONDITION DETAIL &middot; CUFF</span>
    </div>
  </div>

  <aside class="pdp__panel" data-pdp data-needs-size="false" data-preowned="true"
    data-name="${HERO.name}" data-colour="${HERO.colour}" data-price="${u.price}"
    data-img="${u.img}" data-ph="${u.ph}" data-size-fixed="M" data-tier="${t.code}"
    data-seller="${u.seller}" data-eta="${u.eta}">
    <p class="u-eyebrow">${COPY.programme}</p>
    <h1 class="pdp__name">${HERO.name} - ${HERO.colour}</h1>
    <p class="pdp__price">${money(u.price)} <span class="u-strike">${money(HERO.price)}</span></p>
    <p class="pdp__ref">${HERO.colour} ${HERO.ref} &middot; Size M</p>

    <div class="grade">
      <div class="grade__top">
        <span class="unit__tier"><span class="tierdot tierdot--${u.tier}"></span>${t.code}</span>
        <a class="link-u" href="#">How we grade</a>
      </div>
      <p class="u-meta">${t.blurb}</p>
      <div class="checks">
        ${CHECKS.map(([k, v]) => `<div class="check"><span class="check__k">${k}</span><span class="check__v">${v}</span></div>`).join('')}
      </div>
      <p class="u-meta grade__foot">Graded from the owner's photographs and confirmed by Zara on arrival at the store.</p>
    </div>

    <div class="pdp__actions">
      <button class="btn btn--primary btn--block" data-add>${COPY.addToBag}</button>
    </div>

    <ul class="facts-list">
      <li><span>Owner</span><b>${u.seller}</b></li>
      <li><span>Purchased</span><b>${u.worn.replace('Bought ', '')}</b></li>
      <li><span>Original label</span><b>${u.label}</b></li>
      <li><span>Delivery</span><b>${u.eta}</b></li>
    </ul>

    <div class="acc">
      <button class="acc__row"><span class="u-label">${COPY.composition}</span><span class="acc__ico">+</span></button>
      <div class="acc__body">${HERO.composition}<br>Carried over from the original product record.</div>
      <button class="acc__row"><span class="u-label">PRE-OWNED PURCHASE CONDITIONS</span><span class="acc__ico">+</span></button>
    </div>
  </aside>
</main>
${footer()}
${phTemplates()}`;
}

/* ====================================================================== 07
   PRE-OWNED CATALOGUE — with the condition filter Zara does not have today.
   ====================================================================== */
function preowned() {
  const rows = [
    { p: PRODUCTS[0], u: UNITS[0] },
    { p: PRODUCTS[1], u: { tier: 'g', price: 41.0, img: 'dress', ph: 'b' } },
    { p: PRODUCTS[2], u: { tier: 'e', price: 67.0, img: 'skirt', ph: 'c' } },
    { p: PRODUCTS[3], u: { tier: 'f', price: 21.0, img: 'knit', ph: 'e' } },
    { p: PRODUCTS[4], u: { tier: 'g', price: 27.0, img: 'mini', ph: 'f' } },
    { p: PRODUCTS[0], u: UNITS[1] },
    { p: PRODUCTS[0], u: UNITS[2] },
    { p: PRODUCTS[1], u: { tier: 'e', price: 52.0, img: 'dress', ph: 'b' } },
  ];
  const card = ({ p, u }) => {
    const t = tierOf(u.tier);
    return `
    <a class="card" href="preowned-pdp.html">
      <div class="card__media">
        ${pic(u.img, u.ph, p.name)}
        <span class="card__flag card__flag--reloop">${t.code}</span>
      </div>
      <div class="card__info">
        <div class="card__name">${p.name}</div>
        <div class="card__price">${money(u.price)} <span class="u-strike">${money(p.price)}</span></div>
      </div>
    </a>`;
  };

  return `
${header({ nav: 'PRE-OWNED' })}
<section class="po-hero">
  <p class="u-eyebrow">${COPY.programme}</p>
  <h1 class="po-hero__h">${COPY.circularity}</h1>
  <nav class="po-pillars">
    <a href="#" aria-current="page">RESELL</a><a href="#">REPAIR</a><a href="#">DONATION</a>
  </nav>
</section>
<div class="plp-head">
  <span class="plp-title">PRE-OWNED &middot; WOMAN</span>
  <div class="plp-tools"><button>FILTERS</button></div>
</div>
<div class="po-filters">
  <span class="u-eyebrow">CONDITION</span>
  ${TIERS.map((t, i) => `<button class="chip"><span class="tierdot tierdot--${t.key}"></span>${t.code}</button>`).join('')}
  <span class="po-filters__note u-meta">Zara's live catalogue cannot be filtered by condition. Grading makes it possible.</span>
</div>
<main class="grid grid--4">${rows.map(card).join('')}</main>
${footer()}
${phTemplates()}`;
}

/* ====================================================================== 08
   BAG — the real Zara Resell fee structure: the BUYER pays $1 + 5%.
   ====================================================================== */
function bag() {
  return `
${header({ nav: 'WOMAN' })}
<main class="bag">
  <h1 class="u-label bag__h">SHOPPING BAG</h1>
  <div class="bag__grid">
    <div>
      <div data-bag></div>
      ${note('WHAT THE BUYER SEES', `The only difference from an ordinary Zara order is a longer delivery window and a
      line of fees. Checkout, payment and support are the flows Zara already runs.`)}
    </div>

    <aside class="bag__sum" data-sum>
      <h2 class="u-label">ORDER SUMMARY</h2>
      <ul class="sum">
        <li><span>Subtotal</span><b data-sub>$0.00</b></li>
        <li data-fee-row hidden><span>${COPY.buyerFee}</span><b data-fee>$0.00</b></li>
        <li><span>Shipping</span><b data-ship>$4.95</b></li>
      </ul>
      <div class="sum__total"><span>TOTAL</span><b data-total>$0.00</b></div>
      <p class="u-meta sum__note" data-fee-note hidden>The pre-owned transaction fee is $1.00 plus 5% of the item price,
      as on Zara Resell today. The seller receives their full asking price.</p>
      <a class="btn btn--primary btn--block" href="order.html">CONTINUE</a>
    </aside>
  </div>
</main>
${footer()}
${phTemplates()}`;
}

/* ====================================================================== 09 */
function order() {
  const u = UNITS[0];
  return `
${header({ nav: 'WOMAN' })}
<main class="narrow">
  <p class="u-eyebrow">ORDER CONFIRMED</p>
  <h1 class="big">Thank you.</h1>
  <p class="lede">Order 44 118 902 &middot; ${u.eta}. The blazer ships from the Zara store that received and
  confirmed it, on the same label Zara already uses for ship-from-store.</p>
  <div class="facts">
    <div><span class="u-eyebrow">ITEM</span><p>${HERO.name}</p></div>
    <div><span class="u-eyebrow">CONDITION</span><p>${tierOf(u.tier).code}</p></div>
    <div><span class="u-eyebrow">SIZE</span><p>M</p></div>
    <div><span class="u-eyebrow">DELIVERY</span><p>${u.eta}</p></div>
  </div>
  ${note('THE UNIT THAT WAS NOT LOST', `This sale did not exist ten minutes ago. The size was gone, so there was no new
  unit to cannibalise — that revenue was already forfeited. This is the strongest single argument in the pitch, and it
  exists only because the trigger is demand-side.`)}
</main>
${footer()}
${phTemplates()}`;
}

module.exports = { home, plp, pdp, pdpSheet, reloopRequest, preownedPdp, preowned, bag, order };
