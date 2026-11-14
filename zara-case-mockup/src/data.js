'use strict';
/* =========================================================================
   DATA + COPY
   Strings marked [REAL] are verbatim from Zara's live site / help centre /
   Zara Resell T&Cs. Strings marked [NEW] are our proposed feature copy.
   Strings marked [ASSUMED] could not be verified and must be labelled as a
   team assumption in the deck.
   ========================================================================= */

const COPY = {
  // ---- main zara.com, verified ----
  comingSoon: 'Coming soon',                                   // [REAL]
  notifyBody:                                                  // [REAL] help centre
    'You can provide us your email and we will notify you if it becomes available within 15 days. After that, that feature will expire and you will have to register for it again.',
  checkStore: 'Check in-store availability',                   // [REAL]
  addToBag: 'ADD TO BAG',                                      // [REAL by inference]
  findYourSize: 'Find your size',                              // [REAL] named Zara feature, sentence case
  composition: 'COMPOSITION AND CARE',                         // [REAL] help-page title casing
  shipping: 'SHIPPING AND RETURNS',                            // [ASSUMED] panel label unverified
  joinlife: 'JOIN LIFE',                                       // [REAL]

  // ---- Zara Pre-owned / Zara Resell, verified ----
  programme: 'ZARA PRE-OWNED',                                 // [REAL]
  platform: 'ZARA RESELL',                                     // [REAL] platform name
  pillars: ['REPAIR', 'RESELL', 'DONATION'],                   // [REAL] US uses "Second Hand" for Resell
  buySell: 'Buy/Sell',                                         // [REAL] the single combined control
  circularity:                                                 // [REAL] US help centre
    'As part of our commitment to circularity, we want to help our customers lengthen the life of our products.',
  newLife: 'giving Zara pre-owned garments a new life',         // [REAL]
  resellFree: 'Participation in Zara Resell is free of charge for the Selling User.', // [REAL] T&Cs
  resellSteps: [                                               // [REAL] resell.zara.com
    ['LOG IN', 'LOG IN TO ZARA RESELL PLATFORM'],
    ['SUBMIT', "FIND THE ITEM YOU'D LIKE TO SELL FROM OUR PRODUCT CATALOGUE"],
    ['LIST', 'ADD A BRIEF DESCRIPTION, UPLOAD PHOTOS AND SET YOUR PRICE'],
    ['SHIP', 'ONCE YOUR ITEM IS SOLD, USE THE PREPAID DELIVERY AND SHIP IT WITHIN THREE DAYS'],
  ],
  buyerFee: 'Pre-owned transaction fee',                       // [REAL] buyer pays $1 + 5%
  buyerFeeFlat: 1.0,
  buyerFeePct: 0.05,

  // ---- our feature ----
  featureName: 'ZARA RELOOP',                                  // [NEW]
  reloopCta: 'EXPLORE PRE-OWNED',                              // [NEW]
  reloopTagline: 'The size is gone. The garment is not.',      // [NEW]
};

/* Condition tiers. Zara does NOT publish its condition ladder — these three
   tiers are OUR proposal and must be labelled an assumption in the deck.
   The T&Cs bound the ladder at both ends: a top tier gated to "brand new and
   unworn", and a prohibition on listing anything "in very poor condition". */
const TIERS = [                                                // [ASSUMED]
  { key: 'e', code: 'EXCELLENT', pct: 0.75, blurb: 'Worn a handful of times. No visible wear.' },
  { key: 'g', code: 'GOOD',      pct: 0.58, blurb: 'Light, even wear. Shape and colour intact.' },
  { key: 'f', code: 'FAIR',      pct: 0.42, blurb: 'Visible wear, photographed in detail.' },
];

/* The four physical checks the vision model runs. Naming them is what makes
   "AI grades it" auditable to a judge. */
const CHECKS = [
  ['Pilling & surface', 'No pilling detected'],
  ['Seams & structure', 'Intact, shoulder line true'],
  ['Staining & marks', 'None detected'],
  ['Hardware & fastenings', 'All buttons present'],
];

/* ---------------------------------------------------------------- catalogue
   Image slots: drop a real Zara photo at public/assets/img/<img>.jpg and it
   is picked up automatically. Until then a styled placeholder renders. */
/* The hero SKU. Zara's PDP title format is NAME IN CAPS - Colour in sentence
   case, and the reference line is "Colour 2757/397/403". */
const HERO = {
  slug: 'textured-tweed-blazer',
  name: 'TEXTURED TWEED BLAZER',
  colour: 'Blue',
  price: 109.0,
  ref: '2757/397/403',
  desc: 'Regular-fit blazer with a lapel collar, long sleeves, front welt pockets and frayed trims.',
  composition: 'OUTER SHELL: 52% cotton, 26% polyester, 22% acrylic',
  img: 'blazer',
  gallery: ['blazer', 'blazer-2', 'blazer-3'],
  ph: 'a',
  sizes: [
    { s: 'XS', state: 'in' },
    { s: 'S', state: 'in' },
    { s: 'M', state: 'out' },   // the dead end the whole concept hinges on
    { s: 'L', state: 'in' },
    { s: 'XL', state: 'soon' }, // Zara's real second state: "Coming soon"
  ],
};

const PRODUCTS = [
  { name: 'TEXTURED TWEED BLAZER', colour: 'Blue', price: 109.0, img: 'blazer', alt: 'blazer-2', ph: 'a', href: 'pdp.html' },
  { name: 'DRAPED MIDI DRESS', colour: 'Black', price: 69.9, img: 'dress', ph: 'b' },
  { name: 'FRINGED STRIPED SKIRT', colour: 'Multicoloured', price: 89.9, img: 'skirt', ph: 'c' },
  { name: 'CROCHET KNIT TOP', colour: 'Oyster white', price: 49.9, img: 'knit', ph: 'e' },
  { name: 'PLEATED MINI SKIRT', colour: 'Grey marl', price: 45.9, img: 'mini', ph: 'f' },
  { name: 'WOOL BLEND COAT', colour: 'Camel', price: 229.0, img: '', ph: 'g' },
  { name: 'STRAIGHT LEG TROUSERS', colour: 'Black', price: 59.9, img: '', ph: 'd' },
  { name: 'OVERSIZE POPLIN SHIRT', colour: 'White', price: 45.9, img: '', ph: 'h' },
];

/* Matched pre-owned units for the hero SKU in size M. Same garment, different
   owners — so the seller photos are genuinely of the same blazer. */
const UNITS = [
  {
    tier: 'e', price: 82.0, img: 'seller-1', ph: 'a',
    worn: 'Bought March 2026', seller: 'Brooklyn, NY', label: 'Label still attached',
    note: 'Worn twice. No visible wear.', eta: 'Arrives 24–27 Nov',
  },
  {
    tier: 'g', price: 63.0, img: 'seller-2', ph: 'c',
    worn: 'Bought November 2025', seller: 'Queens, NY', label: 'Label removed',
    note: 'Light wear at the cuff, photographed.', eta: 'Arrives 25–28 Nov',
  },
  {
    tier: 'f', price: 46.0, img: 'seller-3', ph: 'e',
    worn: 'Bought August 2025', seller: 'Jersey City, NJ', label: 'Label removed',
    note: 'Pilling on the lining. Shell unmarked.', eta: 'Arrives 25–28 Nov',
  },
];

/* The novelty argument, as data: five systems, only two of them new. */
const SYSTEMS = [
  ['Purchase record', 'exists', 'Zara already knows who bought which SKU, in which size and colour, and when.'],
  ['Zara Pre-Owned', 'exists', 'Live in 17 markets. Repair, Resell and Donation, with a browsable catalogue.'],
  ['Store returns flow', 'exists', 'Garments already travel customer → store every day. We run it in reverse.'],
  ['Ship-from-store', 'exists', 'Zara already fulfils online orders out of store stock.'],
  ['Demand trigger on the PDP', 'new', 'Surface pre-owned at the sold-out size, and notify verified past buyers.'],
  ['Grading & price band', 'new', 'Vision grading into three tiers, priced against observed resale for the same SKU.'],
];

const money = (n) => `$${n.toFixed(2).replace(/\.00$/, '.00')}`;

module.exports = { COPY, TIERS, CHECKS, HERO, PRODUCTS, UNITS, SYSTEMS, money };
