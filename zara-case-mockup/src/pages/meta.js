'use strict';
const L = require('../lib/layout');
const D = require('../data');
const { COPY, SYSTEMS } = D;
const { header, footer, phTemplates, note } = L;

/* ====================================================================== 00
   CONTENTS — the launcher the team clicks from on stage.
   ====================================================================== */
function index(manifest) {
  const group = (title, items) => `
  <div class="ix__group">
    <h2>${title}</h2>
    ${items.map((p) => `
    <a class="ix__item" href="/${p.file}">
      <span class="ix__num">${String(p.step).padStart(2, '0')}</span>
      <span><span class="ix__t">${p.title}</span><span class="ix__d">${p.blurb}</span></span>
      <span class="ix__go">OPEN</span>
    </a>`).join('')}
  </div>`;

  const buyer = manifest.filter((p) => p.group === 'buyer');
  const seller = manifest.filter((p) => p.group === 'seller');
  const argument = manifest.filter((p) => p.group === 'argument');

  return `
<main class="ix">
  <p class="u-eyebrow">TEAM FIREWORKS &middot; BIC x ZARA CASE COMPETITION</p>
  <h1>${COPY.featureName}</h1>
  <p class="ix__lede">${COPY.reloopTagline} A shopper hits a sold-out size, and instead of a dead end Zara asks the
  people who already own that exact garment whether they want to sell it — grading and pricing it for them. Click
  straight through the flow below, or start at step 01 and use the bar at the bottom of every screen.</p>

  ${group('BUYER', buyer)}
  ${group('SELLER', seller)}
  ${group('THE ARGUMENT', argument)}

  <div class="ix__foot">
    <p class="u-meta"><b>What is real and what is ours.</b> The Zara chrome, type, 2:3 imagery, tab list, sold-out
    strikethrough, "Coming soon" state, Pre-Owned pillars and the $1 + 5% buyer fee are reconstructed from Zara's live
    site, help centre and the Zara Resell terms. The demand trigger, the purchase-history match, the guided capture,
    the three-tier grade and the price band are <b>ours</b>. Condition tier names are an assumption — Zara does not
    publish a ladder. Product photography is Zara's own, used to show their UI.</p>
  </div>
</main>
${phTemplates()}`;
}

/* ====================================================================== 17
   THE SYSTEMS MAP — the novelty argument made falsifiable.
   ====================================================================== */
function systems() {
  const exists = SYSTEMS.filter((s) => s[1] === 'exists');
  const isNew = SYSTEMS.filter((s) => s[1] === 'new');
  const row = ([name, state, why]) => `
  <div class="sys ${state}">
    <div class="sys__state">${state === 'exists' ? 'ALREADY RUNS' : 'NEW'}</div>
    <div><div class="sys__n">${name}</div><div class="sys__w">${why}</div></div>
  </div>`;

  return `
${header({ nav: 'WOMAN' })}
<main class="narrow narrow--wide">
  <p class="u-eyebrow">THE NOVELTY ARGUMENT, MADE FALSIFIABLE</p>
  <h1 class="big">Six systems. Four already run in production.</h1>
  <p class="lede">"The pieces exist but nothing is integrated" is true and unfalsifiable as stated. Naming the systems
  and marking which already exist turns it into a claim a Zara judge can check — and it is the line that separates this
  from four other teams pitching a resale app.</p>

  <div class="sys__grid">
    ${exists.map(row).join('')}
    ${isNew.map(row).join('')}
  </div>

  <div class="scoreline"><b>4 of 6</b> <span>already running inside Zara today</span></div>

  ${note('THE CANNIBALISATION ANSWER', `The pre-owned option only appears where the size is sold out, so there is no new
  unit to lose — that sale was already gone. This argument exists only because the trigger is demand-side, and Zara will
  ask it. Put it on the slide rather than saving it for Q&amp;A.`)}

  ${note('THE CATALOGUE ANSWER', `Zara Pre-Owned is already live in 17 markets with repair, resell and donation and a
  browsable page. Say so in one line, then draw the distinction: a catalogue is a destination you have to decide to
  visit, and we are putting the decision inside the flow people are already in.`)}
</main>
${footer()}
${phTemplates()}`;
}

module.exports = { index, systems };
