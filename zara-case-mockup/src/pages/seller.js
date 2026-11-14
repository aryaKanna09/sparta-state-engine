'use strict';
const L = require('../lib/layout');
const D = require('../data');
const { COPY, TIERS, CHECKS, HERO, UNITS, money } = D;
const { header, footer, pic, phTemplates, note, phone, appBar, tabbar, steps } = L;

const tierOf = (k) => TIERS.find((t) => t.key === k);

/* A page that presents one or more phone screens on a neutral stage. */
function stage({ eyebrow, title, lede, screens, after = '' }) {
  return `
${header({ nav: 'WOMAN' })}
<main class="stage">
  <div class="stage__intro">
    <p class="u-eyebrow">${eyebrow}</p>
    <h1 class="big">${title}</h1>
    <p class="lede">${lede}</p>
  </div>
  <div class="phones">${screens}</div>
  ${after}
</main>
${footer()}
${phTemplates()}`;
}

/* ====================================================================== 10
   THE NOTIFICATION — demand reaches the person already holding the supply.
   ====================================================================== */
function notify() {
  const lock = `
  <div class="lock">
    <div class="lock__clock"><b>9:41</b><span>Friday, 14 November</span></div>
    <div class="push">
      <div class="push__icon"><span class="wordmark">Z</span></div>
      <div>
        <div class="push__t">ZARA</div>
        <div class="push__b">3 people are looking for the tweed blazer you bought in March, in your size. Want to sell it?</div>
      </div>
      <div class="push__time">now</div>
    </div>
    <div class="push">
      <div class="push__icon"><span class="wordmark">Z</span></div>
      <div>
        <div class="push__t">ZARA</div>
        <div class="push__b">Your order 44 118 902 has shipped.</div>
      </div>
      <div class="push__time">2h</div>
    </div>
  </div>`;

  return stage({
    eyebrow: `${COPY.featureName} &middot; SELLER`,
    title: 'The ask arrives where the garment already is.',
    lede: `Zara knows who bought this blazer, in this size and colour, and when. Nobody has to go looking for a
      listing to create — the demand finds the owner.`,
    screens: phone({ screen: lock, label: 'PUSH NOTIFICATION' }),
    after: note('THE WEDGE', `Zara Resell today is <b>catalogue-matched</b>: you have to find your own item in the product
      catalogue, and sellers report that search degrades for older pieces. It never uses your order history. Matching on
      the purchase record instead is what makes ownership and authenticity free, and it is genuinely not what Zara does today.`),
  });
}

/* ====================================================================== 11
   THE CLOSET — purchase history, re-read as sellable supply.
   ====================================================================== */
function closet() {
  const row = (name, bought, demand, img, ph) => `
  <div class="row">
    <div class="row__media">${pic(img, ph, name)}</div>
    <div>
      <div class="row__t">${name}</div>
      <div class="row__s">${bought}</div>
      ${demand ? `<div style="margin-top:6px"><span class="demand">${demand}</span></div>` : ''}
    </div>
    <div class="row__a">${demand ? 'SELL' : ''}</div>
  </div>`;

  const screen = `
  ${appBar({ title: 'MY PURCHASES' })}
  <div class="app-body">
    <p class="u-eyebrow" style="margin-bottom:10px">WANTED RIGHT NOW</p>
    ${row(HERO.name, 'Bought 14 March 2026 &middot; Size M', '3 PEOPLE WANT THIS', 'blazer', 'a')}
    ${row('DRAPED MIDI DRESS', 'Bought 2 Feb 2026 &middot; Size M', '1 PERSON WANTS THIS', 'dress', 'b')}
    <p class="u-eyebrow" style="margin:22px 0 10px">EVERYTHING ELSE</p>
    ${row('CROCHET KNIT TOP', 'Bought 9 Oct 2025 &middot; Size M', null, 'knit', 'e')}
    ${row('PLEATED MINI SKIRT', 'Bought 4 Aug 2025 &middot; Size M', null, 'mini', 'f')}
  </div>
  ${tabbar('Account')}`;

  return stage({
    eyebrow: `${COPY.featureName} &middot; SELLER`,
    title: 'Your order history, read as supply.',
    lede: `Every Zara purchase is already in the app under My Purchases. Attaching live demand to it turns a receipt
      archive into an inventory list — and the items people actually want rise to the top.`,
    screens: phone({ screen, label: 'PURCHASE HISTORY WITH DEMAND' }),
    after: note('AUTHENTICATION IS FREE HERE', `Ownership and authenticity are proven before a photo is taken, because the
      order is already in the account. Guest-checkout buyers verify with the receipt scan Zara's app already supports
      ("Add Receipt"). That is why this costs Zara almost nothing per unit.`),
  });
}

/* ====================================================================== 12
   GUIDED CAPTURE — the app coaches the photograph.
   ====================================================================== */
function capture() {
  const cam = (n, label, coach, on) => `
  ${appBar({ title: 'PHOTOGRAPH THE ITEM' })}
  ${steps(n, 4)}
  <div class="cam">
    <div class="cam__shot">${pic('capture', 'a', 'Camera view')}</div>
    <div class="cam__frame"></div>
    <div class="cam__steps">${[1, 2, 3, 4].map((i) => `<i class="${i <= on ? 'on' : ''}"></i>`).join('')}</div>
    <div class="cam__coach"><b>${label}</b><p>${coach}</p></div>
    <div class="cam__shutter"></div>
  </div>`;

  return stage({
    eyebrow: `${COPY.featureName} &middot; SELLER`,
    title: 'No listing to write. Four photographs.',
    lede: `The app frames each shot and coaches the lighting, because the grade is only as good as the photograph.
      The Zara tag is one of the four, which is what lets the model read the product code off the garment itself.`,
    screens: [
      phone({ screen: cam(1, 'FRONT, FLAT, DAYLIGHT', 'Lay the blazer flat near a window. Avoid overhead light.', 1), label: '01 FRONT' }),
      phone({ screen: cam(2, 'BACK', 'Same distance. Keep the whole garment inside the frame.', 2), label: '02 BACK' }),
      phone({ screen: cam(3, 'THE ZARA TAG', 'Hold steady until the product code is sharp.', 3), label: '03 TAG' }),
      phone({ screen: cam(4, 'ANY WEAR, UP CLOSE', 'Photograph cuffs, seams and lining. Flaws raise trust, not risk.', 4), label: '04 DETAIL' }),
    ].join(''),
    after: note('WHY COACHING MATTERS', `Grading accuracy is a photography problem before it is a model problem.
      Coaching the capture is what keeps the re-grade on arrival from disagreeing with the offer — and a disagreement
      is the one thing that would break seller trust.`),
  });
}

/* ====================================================================== 13
   GRADING + THE OFFER.
   ====================================================================== */
function offer() {
  const t = tierOf('e');
  const price = 82.0;
  // The band is the observed resale spread for this product code, not the
  // tier range — so the offer sits inside it rather than pinned to its own top.
  const lo = 38;
  const hi = 92;
  const pos = ((price - lo) / (hi - lo)) * 100;

  const grading = `
  ${appBar({ title: 'GRADING' })}
  <div class="app-body app-body--center">
    <div class="spinner"></div>
    <p class="u-label" style="margin-top:22px">READING YOUR PHOTOGRAPHS</p>
    <ul class="ticks">
      ${CHECKS.map(([k], i) => `<li class="${i < 3 ? 'done' : ''}">${k}</li>`).join('')}
    </ul>
    <p class="u-meta" style="margin-top:20px">Usually under a minute.</p>
  </div>`;

  const result = `
  ${appBar({ title: 'YOUR OFFER' })}
  <div class="app-body">
    <div class="offer">
      <div class="offer__tier"><span class="tierdot tierdot--e"></span>${t.code}</div>
      <div class="offer__amt">${money(price)}</div>
      <div class="offer__sub">You receive all of it. Zara takes no seller fee.</div>
    </div>

    <div class="band">
      <div class="band__track">
        <div class="band__fill" style="left:0;width:${pos}%"></div>
        <div class="band__pin" style="left:${pos}%"></div>
      </div>
      <div class="band__ends"><span>${money(lo)} LOWEST</span><span>YOUR OFFER ${money(price)}</span><span>${money(hi)} HIGHEST</span></div>
    </div>
    <p class="u-meta">Priced against what this exact product code has actually resold for over the last 90 days,
      with ${money(HERO.price)} original retail as the ceiling.</p>

    <div class="checks" style="margin-top:20px">
      ${CHECKS.map(([k, v]) => `<div class="check"><span class="check__k">${k}</span><span class="check__v">${v}</span></div>`).join('')}
    </div>
    <a class="link-u" href="#" style="display:inline-block;margin-top:14px">See what the model looked at</a>
  </div>
  <div class="app-foot">
    <a class="btn btn--primary btn--block" href="seller-dropoff.html">ACCEPT ${money(price)}</a>
    <div class="foot-row">
      <a class="link-u" href="seller-appeal.html">Appeal the grade</a>
      <a class="link-u" href="#">Decline</a>
    </div>
  </div>`;

  return stage({
    eyebrow: `${COPY.featureName} &middot; SELLER`,
    title: 'A grade you can argue with, and a price you can check.',
    lede: `Three tiers, four named physical checks, and a band anchored to observed resale for the same product code.
      The seller approves, declines or appeals — they never write a listing or guess a price.`,
    screens: phone({ screen: grading, label: 'GRADING' }) + phone({ screen: result, label: 'GRADE AND PRICE' }),
    after: note('LABEL THIS AN ASSUMPTION', `Zara does not publish a condition ladder, so EXCELLENT / GOOD / FAIR is
      <b>our proposal</b>, not Zara's copy — mark it as a team assumption on the slide. What is real: the Zara Resell
      T&amp;Cs gate the top of the ladder to "brand new and unworn" and forbid listing anything "in very poor condition",
      so a bounded three-tier scale sits correctly inside their own terms.`),
  });
}

/* ====================================================================== 14 */
function appeal() {
  const screen = `
  ${appBar({ title: 'APPEAL THE GRADE' })}
  <div class="app-body">
    <p class="u-meta">We graded this <b>EXCELLENT</b>. Tell us what we read wrong and add a photograph of it.</p>
    <div class="choices">
      ${['The lining is unmarked', 'The label is still attached', 'There is no pilling', 'Something else'].map(
        (c, i) => `<label class="choice${i === 1 ? ' is-on' : ''}"><span>${c}</span><i></i></label>`).join('')}
    </div>
    <div class="uploader"><span class="u-label">ADD A PHOTOGRAPH</span><span class="uploader__plus">+</span></div>
    <p class="u-meta">A person reviews appeals within one working day. If we agree, the offer is reissued at the higher tier.</p>
  </div>
  <div class="app-foot"><button class="btn btn--primary btn--block">SUBMIT APPEAL</button></div>`;

  return stage({
    eyebrow: `${COPY.featureName} &middot; SELLER`,
    title: 'The appeal is the trust mechanism.',
    lede: `An automated grade that cannot be challenged is a grade sellers stop believing. The appeal path, and a
      published tolerance for what happens when Zara re-grades on arrival, is what keeps the offer credible.`,
    screens: phone({ screen, label: 'APPEAL' }),
    after: note('THE TOLERANCE, STATED', `Zara re-grades on arrival. If the tier drops, the seller takes a revised offer
      or the garment back, shipped free. Saying this out loud on the slide closes the obvious question before a judge asks it.`),
  });
}

/* ====================================================================== 15 */
function dropoff() {
  const screen = `
  ${appBar({ title: 'GETTING IT TO US' })}
  <div class="app-body">
    <label class="opt is-on">
      <div><div class="opt__t">Drop it at a Zara store</div>
      <div class="opt__s">Zara SoHo &middot; 580 Broadway &middot; 0.4 mi<br>Open until 9pm. No packaging needed.</div></div>
      <i></i>
    </label>
    <label class="opt">
      <div><div class="opt__t">Ship it free</div>
      <div class="opt__s">Prepaid label by email. Drop at any carrier point within three days.</div></div>
      <i></i>
    </label>
    <div class="flowmap">
      <p class="u-eyebrow">WHERE THE GARMENT GOES</p>
      <ol class="flowmap__l">
        <li>You &rarr; store <span>existing returns flow, run in reverse</span></li>
        <li>Store confirms the grade <span>one grading touch</span></li>
        <li>Store &rarr; buyer <span>existing ship-from-store</span></li>
      </ol>
      <p class="u-meta">No new distribution centre. No new carrier. The delta against a normal order is one inbound
      leg and one grading touch.</p>
    </div>
  </div>
  <div class="app-foot"><button class="btn btn--primary btn--block">CONFIRM DROP-OFF</button></div>`;

  return stage({
    eyebrow: `${COPY.featureName} &middot; SELLER`,
    title: 'It runs on flows Zara already operates.',
    lede: `This is the question every judge asks, so it is answered on the screen: we are not building peer-to-peer
      logistics. The garment enters through store returns and leaves on ship-from-store.`,
    screens: phone({ screen, label: 'DROP-OFF OR SHIP' }),
  });
}

/* ====================================================================== 16 */
function status() {
  const n = (t, s, cls) => `<div class="track__n ${cls}"><div class="track__d"></div><div><div class="track__t">${t}</div><div class="track__s">${s}</div></div></div>`;
  const screen = `
  ${appBar({ title: 'MY SALES' })}
  <div class="app-body">
    <div class="row" style="border:0;padding-top:0">
      <div class="row__media">${pic('seller-1', 'a', HERO.name)}</div>
      <div>
        <div class="row__t">${HERO.name}</div>
        <div class="row__s">Size M &middot; EXCELLENT &middot; ${money(82)}</div>
      </div>
      <div class="row__a"></div>
    </div>
    <div class="track">
      ${n('Graded and accepted', '14 Nov, 9:48', 'done')}
      ${n('Dropped at Zara SoHo', '14 Nov, 6:20pm', 'done')}
      ${n('Grade confirmed by the store', '15 Nov, 10:02', 'done')}
      ${n('Sold', 'To a buyer who wanted size M', 'now')}
      ${n('Paid out', `${money(82)} via Stripe, 2–3 working days`, '')}
    </div>
    ${['', ''].length ? '' : ''}
    <div class="payout">
      <span class="u-eyebrow">YOU RECEIVE</span>
      <div class="payout__amt">${money(82)}</div>
      <p class="u-meta">${COPY.resellFree} The buyer pays the $1.00 + 5% transaction fee.</p>
    </div>
  </div>
  ${tabbar('Account')}`;

  return stage({
    eyebrow: `${COPY.featureName} &middot; SELLER`,
    title: 'Five states, and the seller never answers a message.',
    lede: `No listing to maintain, no haggling, no buyer questions. The seller's entire job was four photographs and
      one tap on an offer.`,
    screens: phone({ screen, label: 'SELLER STATUS AND PAYOUT' }),
    after: note('THE SELLER FRICTION NUMBER', `On Zara Resell today the seller writes the description, sets the price with
      no guidance, waits for approval, and then answers buyers. Here the work is four photographs and one decision.
      That difference is what slide three has to close with a number.`),
  });
}

module.exports = { notify, closet, capture, offer, appeal, dropoff, status };
