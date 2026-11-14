'use strict';
const L = require('../lib/layout');
const D = require('../data');
const { COPY } = D;
const { header, footer, pic, phTemplates, note } = L;

const CHECKS = D.CHECKS;

/* ====================================================================== 10
   THE SELLER JOURNEY — one app, every step tappable.
   ====================================================================== */
function sellerApp() {
  const bar = (title, right = '') => `
    <div class="app-bar">
      <button class="app-bar__back" id="back" data-act="back">&#8249;</button>
      <span class="app-bar__title">${title}</span>
      <span class="app-bar__title app-bar__right">${right}</span>
    </div>`;

  const row = (name, sub, demand, img, ph, go) => `
    <div class="row${go ? ' row--tap' : ''}"${go ? ` data-go="${go}"` : ''}>
      <div class="row__media">${pic(img, ph, name)}</div>
      <div>
        <div class="row__t">${name}</div>
        <div class="row__s">${sub}</div>
        ${demand ? `<div style="margin-top:6px"><span class="demand">${demand}</span></div>` : ''}
      </div>
      <div class="row__a">${go ? 'SELL' : ''}</div>
    </div>`;

  const track = (t, s, cls) =>
    `<div class="track__n ${cls}"><div class="track__d"></div><div><div class="track__t">${t}</div><div class="track__s">${s}</div></div></div>`;

  return `
${header({ nav: 'WOMAN' })}
<main class="app-stage">
  <div class="app-stage__head">
    <p class="u-eyebrow">${COPY.featureName} &middot; SELLER</p>
    <h1 class="app-stage__h">Tap through the whole seller journey.</h1>
    <p class="u-meta">Notification &rarr; your closet &rarr; four coached photographs &rarr; grade and price &rarr; drop-off &rarr; payout.
      The camera and the capture coaching are real. The grade is simulated.</p>
  </div>

  <div class="app-stage__phone">
    <div class="phone phone--live">
      <div class="phone__cap"><span>9:41</span><span class="phone__sig"><i></i><i></i><i></i><i></i></span></div>
      <div class="phone__screen">

        <!-- ------------------------------------------------------ lock -->
        <section id="v-lock" class="sv">
          <div class="lock">
            <div class="lock__clock"><b>9:41</b><span>Friday, 14 November</span></div>
            <button class="push push--tap" data-go="closet">
              <div class="push__icon"><span class="wordmark">Z</span></div>
              <div>
                <div class="push__t">ZARA</div>
                <div class="push__b">3 people are looking for the tweed blazer you bought in March, in your size. Want to sell it?</div>
              </div>
              <div class="push__time">now</div>
            </button>
            <p class="lock__hint">Tap the notification</p>
          </div>
        </section>

        <!-- ---------------------------------------------------- closet -->
        <section id="v-closet" class="sv" hidden>
          ${bar('MY PURCHASES')}
          <div class="app-body">
            <p class="u-eyebrow" style="margin-bottom:8px">WANTED RIGHT NOW</p>
            ${row('TEXTURED TWEED BLAZER', 'Bought 14 March 2026 &middot; Size M', '3 PEOPLE WANT THIS', 'blazer', 'a', 'intro')}
            ${row('DRAPED MIDI DRESS', 'Bought 2 Feb 2026 &middot; Size M', '1 PERSON WANTS THIS', 'dress', 'b', 'intro')}
            <p class="u-eyebrow" style="margin:20px 0 8px">EVERYTHING ELSE</p>
            ${row('CROCHET KNIT TOP', 'Bought 9 Oct 2025 &middot; Size M', null, 'knit', 'e', null)}
            ${row('PLEATED MINI SKIRT', 'Bought 4 Aug 2025 &middot; Size M', null, 'mini', 'f', null)}
          </div>
        </section>

        <!-- ----------------------------------------------------- intro -->
        <section id="v-intro" class="sv" hidden>
          ${bar('SELL THIS ITEM')}
          <div class="app-body app-body--center">
            <div class="glens"></div>
            <p class="u-label" style="margin-top:18px">FOUR PHOTOGRAPHS</p>
            <p class="u-meta" style="margin-top:8px;max-width:27ch">
              Front, back, the Zara tag, and any wear. We coach each one, then grade it.
              Your order is already in your account, so ownership is proven before you shoot.</p>
            <p id="camnote" class="u-meta" style="margin-top:12px;color:var(--grey-400)"></p>
          </div>
          <div class="app-foot">
            <button class="btn btn--primary btn--block" data-act="camera">USE MY CAMERA</button>
            <button class="btn btn--quiet btn--block" data-act="sample">USE THE SAMPLE GARMENT</button>
          </div>
        </section>

        <!-- --------------------------------------------------- capture -->
        <section id="v-capture" class="sv" hidden>
          ${bar('PHOTOGRAPH THE ITEM', '<span id="cstep">01 / 04</span>')}
          <div class="cam cam--live">
            <video id="cam" playsinline muted autoplay></video>
            <img id="still" alt="" style="display:none">
            <div class="cam__frame"></div>
            <div class="cam__steps" id="cdots"><i class="on"></i><i></i><i></i><i></i></div>
            <div class="gchips">
              <span class="gchip" id="chip-light">LIGHT</span>
              <span class="gchip" id="chip-sharp">SHARP</span>
              <span class="gchip" id="chip-frame">FRAME</span>
            </div>
            <div class="cam__coach">
              <b id="clabel">FRONT, FLAT, DAYLIGHT</b>
              <p id="ccoach">Lay the blazer flat near a window.</p>
              <p id="readout" class="greadout"></p>
            </div>
            <div class="gthumbs" id="thumbs"></div>
            <button class="cam__shutter" id="shutter" data-act="shoot" aria-label="Take photograph"></button>
          </div>
        </section>

        <!-- --------------------------------------------------- grading -->
        <section id="v-grading" class="sv" hidden>
          ${bar('GRADING')}
          <div class="app-body app-body--center">
            <div class="spinner"></div>
            <p class="u-label" style="margin-top:20px">READING YOUR PHOTOGRAPHS</p>
            <ul class="ticks" id="ticks">${CHECKS.map(([k]) => `<li>${k}</li>`).join('')}</ul>
          </div>
        </section>

        <!-- ----------------------------------------------------- offer -->
        <section id="v-offer" class="sv" hidden>
          ${bar('YOUR OFFER')}
          <div class="app-body">
            <div class="offer">
              <div class="offer__tier" id="otier"></div>
              <div class="offer__amt" id="oamt"></div>
              <div class="offer__sub" id="oblurb"></div>
            </div>
            <div class="band">
              <div class="band__track"><div class="band__fill" id="ofill" style="left:0"></div><div class="band__pin" id="opin"></div></div>
              <div class="band__ends"><span id="oband"></span></div>
            </div>
            <p class="u-meta" id="oqual"></p>
            <div class="gshots" id="oshots"></div>
            <div class="checks" style="margin-top:14px">
              ${CHECKS.map(([k, v]) => `<div class="check"><span class="check__k">${k}</span><span class="check__v">${v}</span></div>`).join('')}
            </div>
            <p class="u-meta" style="margin-top:10px">You receive the full amount. Zara takes no seller fee.</p>
          </div>
          <div class="app-foot">
            <button class="btn btn--primary btn--block" id="oaccept" data-go="dropoff">ACCEPT</button>
            <div class="foot-row">
              <button class="link-u" data-go="appeal">Appeal the grade</button>
              <button class="link-u" data-act="reset">Decline</button>
            </div>
          </div>
        </section>

        <!-- ---------------------------------------------------- appeal -->
        <section id="v-appeal" class="sv" hidden>
          ${bar('APPEAL THE GRADE')}
          <div class="app-body">
            <p class="u-meta">Tell us what we read wrong and add a photograph of it.</p>
            <div class="choices">
              ${['The lining is unmarked', 'The label is still attached', 'There is no pilling', 'Something else']
                .map((c, i) => `<button class="choice${i === 1 ? ' is-on' : ''}"><span>${c}</span><i></i></button>`).join('')}
            </div>
            <div class="uploader"><span class="u-label">ADD A PHOTOGRAPH</span><span class="uploader__plus">+</span></div>
            <p class="u-meta">A person reviews appeals within one working day. If we agree, the offer is reissued at the higher tier.</p>
          </div>
          <div class="app-foot"><button class="btn btn--primary btn--block" data-act="appeal-ok">SUBMIT APPEAL</button></div>
        </section>

        <!-- --------------------------------------------------- dropoff -->
        <section id="v-dropoff" class="sv" hidden>
          ${bar('GETTING IT TO US')}
          <div class="app-body">
            <button class="opt is-on">
              <div><div class="opt__t">Drop it at a Zara store</div>
              <div class="opt__s">Zara SoHo &middot; 580 Broadway &middot; 0.4 mi<br>Open until 9pm. No packaging needed.</div></div><i></i>
            </button>
            <button class="opt">
              <div><div class="opt__t">Ship it free</div>
              <div class="opt__s">Prepaid label by email. Drop at any carrier point within three days.</div></div><i></i>
            </button>
            <div class="flowmap">
              <p class="u-eyebrow">WHERE THE GARMENT GOES</p>
              <ol class="flowmap__l">
                <li>You &rarr; store <span>existing returns flow, run in reverse</span></li>
                <li>Store confirms the grade <span>one grading touch</span></li>
                <li>Store &rarr; buyer <span>existing ship-from-store</span></li>
              </ol>
              <p class="u-meta">No new distribution centre. No new carrier.</p>
            </div>
          </div>
          <div class="app-foot"><button class="btn btn--primary btn--block" data-go="status">CONFIRM DROP-OFF</button></div>
        </section>

        <!-- ---------------------------------------------------- status -->
        <section id="v-status" class="sv" hidden>
          ${bar('MY SALES')}
          <div class="app-body">
            <div class="row" style="border:0;padding-top:0">
              <div class="row__media">${pic('seller-1', 'a', 'Blazer')}</div>
              <div><div class="row__t">TEXTURED TWEED BLAZER</div>
                <div class="row__s">Size M &middot; <span id="statustier">EXCELLENT</span> &middot; <span id="statusprice">$82.00</span></div></div>
              <div class="row__a"></div>
            </div>
            <div class="track">
              ${track('Graded and accepted', 'Just now', 'done')}
              ${track('Dropped at Zara SoHo', 'Today, 6:20pm', 'done')}
              ${track('Grade confirmed by the store', 'Tomorrow, 10:02', 'done')}
              ${track('Sold', 'To a buyer who wanted size M', 'now')}
              ${track('Paid out', 'via Stripe, 2–3 working days', '')}
            </div>
            <div class="payout">
              <span class="u-eyebrow">YOU RECEIVE</span>
              <div class="payout__amt" id="payamt">$82.00</div>
              <p class="u-meta">${COPY.resellFree} The buyer pays the $1.00 + 5% transaction fee.</p>
            </div>
          </div>
          <div class="app-foot"><button class="btn btn--quiet btn--block" data-act="reset">RUN IT AGAIN</button></div>
        </section>

      </div>
    </div>
    <div class="phone__label">TAP THROUGH IT<span id="forcedtag" hidden class="glock"></span></div>
  </div>

  ${note('RUNNING IT ON STAGE', `Tap the notification to begin. At the photograph step the three chips go black when
    exposure, sharpness and framing actually pass &mdash; that part is measuring the frame. <b>Press 1, 2 or 3 to lock the
    tier</b> to EXCELLENT, GOOD or FAIR before the fourth shot, and Space fires the shutter. No camera, or on a page that
    blocks it? Choose <b>Use the sample garment</b> and the whole flow still runs.`)}

  ${note('THE WEDGE', `Zara Resell today is <b>catalogue-matched</b>: you have to find your own item in the product
    catalogue, and search degrades for older pieces. It never uses your order history. Matching on the purchase record
    is what makes ownership and authenticity free &mdash; and it is genuinely not what Zara does today.`)}
</main>
${footer()}
${phTemplates()}`;
}

module.exports = { sellerApp };
