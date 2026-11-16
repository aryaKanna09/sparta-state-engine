// ZARA ReLoop - BIC x ZARA case competition, Round 1 deck.
// Brand: black / white / greys. Headings in Bodoni Moda (Didone, the style of Zara's wordmark);
// body in Arial (neo-grotesque, close to Zara's UI). Strict grid: 0.5" margins, 0.2" gutters.
const pptxgen = require("pptxgenjs");
const D = __dirname + "/";
const IMG = D + "img/";

const BLACK = "111111", INK = "1A1A1A", G9 = "2B2B2B", G8 = "3D3D3D", G6 = "6B6B6B", G5 = "8A8A8A",
  G4 = "B5B5B5", G3 = "D6D6D6", G2 = "E8E8E8", G1 = "F4F4F4", WHITE = "FFFFFF";
const HEAD = "Bodoni Moda", BODY = "Arial";
const W = 13.333, H = 7.5, M = 0.5, GUT = 0.2, CW = W - 2 * M;
const TOP = 1.6, MAIN_BOTTOM = 5.95, TK_Y = 6.1, TK_H = 0.58, APX_BOTTOM = 6.62;
const PHONE = 1900 / 860, REAL = 2868 / 1320, LOGO = 139 / 361;
const LS = 1.12;

const pres = new pptxgen();
pres.layout = "LAYOUT_WIDE";
pres.title = "ZARA ReLoop";
pres.author = "Team Fireworks";
pres.theme = { headFontFace: HEAD, bodyFontFace: BODY };

const shadow = () => ({ type: "outer", color: "000000", blur: 8, offset: 2, angle: 90, opacity: 0.14 });
const cols = (n, x0 = M, w0 = CW) => { const w = (w0 - GUT * (n - 1)) / n; return Array.from({ length: n }, (_, i) => ({ x: x0 + i * (w + GUT), w })); };
const SECTIONS = ["PROBLEM", "RELOOP", "INSIGHTS", "ECONOMICS", "PILOT"];
let page = 1;

const stop = t => { const s = t.trim(); return /[.!?]$/.test(s) || /\*\*$/.test(s) && /[.!?]\*\*$/.test(s) ? s : (/\*\*$/.test(s) ? s.replace(/\*\*$/, ".**") : s + "."); };

function txt(s, t, o) { s.addText(t, Object.assign({ fontFace: BODY, color: INK, margin: 0, isTextBox: true, valign: "top", lineSpacingMultiple: LS }, o)); }
function rich(str, base = {}) {
  const bc = base.boldColor || BLACK; const b = Object.assign({}, base); delete b.boldColor;
  const out = [];
  str.split("**").forEach((p, i) => { if (p) out.push({ text: p, options: Object.assign({}, b, i % 2 ? { bold: true, color: bc } : {}) }); });
  return out;
}
function body(s, str, o) {
  const run = rich(stop(str), { fontSize: o.fontSize || 11, color: o.color || G9, boldColor: o.boldColor });
  const box = Object.assign({}, o); delete box.fontSize; delete box.color; delete box.boldColor;
  txt(s, run, box);
}
function bullets(s, items, x, y, w, h, size = 11, gap = 6) {
  const runs = [];
  items.forEach((it, i) => {
    const r = rich(stop(it), { fontSize: size, color: G9 });
    r[0].options = Object.assign({}, r[0].options, { bullet: { indent: 12 }, paraSpaceAfter: gap });
    r[r.length - 1].options = Object.assign({}, r[r.length - 1].options, { breakLine: i < items.length - 1 });
    runs.push(...r);
  });
  txt(s, runs, { x, y, w, h });
}
function label(s, t, x, y, w, color = G6, align = "left") { txt(s, t, { x, y, w, h: 0.22, fontSize: 9, bold: true, color, charSpacing: 2.5, align }); }

function frame(s, sec, kicker, title, sources, appendix) {
  s.background = { color: WHITE };
  label(s, kicker, M, 0.34, 9);
  txt(s, title, { x: M, y: 0.6, w: 11.0, h: 0.86, fontFace: HEAD, fontSize: 22, color: BLACK, lineSpacingMultiple: 1.0 });
  s.addImage({ path: D + "logo_black.png", x: W - M - 0.85, y: 0.36, w: 0.85, h: 0.85 * LOGO });
  if (sources) txt(s, "Sources: " + sources, { x: M, y: 6.76, w: CW, h: 0.3, fontSize: 7.5, color: G5, lineSpacingMultiple: 1.0 });
  const names = appendix ? ["APPENDIX"] : SECTIONS;
  names.forEach((n, i) => {
    const on = appendix || i === sec;
    txt(s, (appendix ? "" : "0" + (i + 1) + "  ") + n, { x: M + i * 1.65, y: 7.12, w: 1.6, h: 0.2, fontSize: 7.5, bold: on, color: on ? BLACK : G4, charSpacing: 2 });
  });
  page += 1;
  txt(s, String(page), { x: W - M - 0.5, y: 7.12, w: 0.5, h: 0.2, fontSize: 8, color: G6, align: "right" });
}
function card(s, x, y, w, h, head, opts = {}) {
  s.addShape(pres.shapes.RECTANGLE, { x, y, w, h, fill: { color: opts.fill || WHITE }, line: { color: G3, width: 0.75 }, shadow: shadow() });
  if (head) {
    s.addShape(pres.shapes.RECTANGLE, { x, y, w, h: 0.4, fill: { color: BLACK }, line: { color: BLACK, width: 0.75 } });
    txt(s, head, { x: x + 0.18, y, w: w - 0.36, h: 0.4, fontFace: HEAD, fontSize: 12, bold: true, color: WHITE, charSpacing: 1, valign: "middle", lineSpacingMultiple: 1.0 });
  }
  return { x: x + 0.18, y: y + (head ? 0.58 : 0.18), w: w - 0.36, h: h - (head ? 0.58 : 0.18) - 0.15 };
}
function takeaway(s, text, y = TK_Y) {
  s.addShape(pres.shapes.RECTANGLE, { x: M, y, w: CW, h: TK_H, fill: { color: BLACK }, line: { color: BLACK }, shadow: shadow() });
  txt(s, "SO WHAT", { x: M + 0.2, y, w: 1.15, h: TK_H, fontSize: 9, bold: true, color: G4, charSpacing: 2.5, valign: "middle" });
  txt(s, rich(stop(text), { fontSize: 12, color: G2, boldColor: WHITE }), { x: M + 1.4, y, w: CW - 1.6, h: TK_H, valign: "middle" });
}
function frameImg(s, path, x, y, w, h) {
  s.addShape(pres.shapes.ROUNDED_RECTANGLE, { x: x - 0.05, y: y - 0.05, w: w + 0.1, h: h + 0.1, rectRadius: 0.1, fill: { color: BLACK }, line: { color: G8, width: 0.75 }, shadow: shadow() });
  s.addImage({ path, x, y, w, h });
}
function phoneIn(s, file, slotX, slotW, y, h, cap, ratio = PHONE, capColor = G8) {
  const w = h / ratio, x = slotX + (slotW - w) / 2;
  frameImg(s, IMG + file, x, y, w, h);
  if (cap) txt(s, stop(cap), { x: slotX, y: y + h + 0.14, w: slotW, h: 0.5, fontSize: 9, color: capColor, align: "center" });
}
function stat(s, x, y, w, big, small, dark) {
  txt(s, big, { x, y, w, h: 0.6, fontFace: HEAD, fontSize: 28, bold: true, color: dark ? WHITE : BLACK, lineSpacingMultiple: 1.0 });
  txt(s, stop(small), { x, y: y + 0.66, w, h: 0.6, fontSize: 9.5, color: dark ? G3 : G8 });
}
function table(s, rows, x, y, w, colW, o = {}) {
  const fs = o.fs || 10;
  const data = rows.map((r, ri) => r.map((c, ci) => {
    const head = ri === 0, hl = o.boldRow === ri;
    return { text: c, options: { fontFace: head ? HEAD : BODY, fontSize: head ? fs + 0.5 : fs, color: head ? WHITE : G9, bold: head || hl || (o.boldFirst && ci === 0),
      fill: { color: head ? BLACK : (hl ? G2 : (ri % 2 ? WHITE : G1)) }, valign: "middle", margin: [4, 8, 4, 8], align: ci > 0 && o.right ? "right" : "left" } };
  }));
  s.addTable(data, { x, y, w, colW, border: { type: "solid", pt: 0.5, color: G3 }, rowH: o.rowH || 0.32, autoPage: false });
}

/* ========================= TITLE ========================= */
{
  const s = pres.addSlide();
  s.background = { color: BLACK };
  s.addShape(pres.shapes.RECTANGLE, { x: 0, y: 0, w: 7.4, h: H, fill: { color: "161616" }, line: { color: "161616" } });
  s.addImage({ path: D + "logo_white.png", x: 0.8, y: 0.8, w: 2.2, h: 2.2 * LOGO });
  label(s, "BIC x ZARA CASE COMPETITION  ·  ROUND 1", 0.8, 1.95, 6.2, G4);
  txt(s, "ReLoop", { x: 0.8, y: 2.35, w: 6.2, h: 1.2, fontFace: HEAD, fontSize: 66, color: WHITE, lineSpacingMultiple: 1.0 });
  txt(s, "Making pre-owned as easy to choose as new: native in the Zara app, graded by AI, verified by Zara before it ships.", { x: 0.8, y: 3.65, w: 6.0, h: 1.0, fontSize: 15, color: G2 });
  s.addShape(pres.shapes.RECTANGLE, { x: 0.8, y: 5.2, w: 6.0, h: 1.4, fill: { color: "222222" }, line: { color: G8, width: 0.75 }, shadow: shadow() });
  txt(s, "TEAM FIREWORKS", { x: 1.05, y: 5.38, w: 5.5, h: 0.3, fontFace: HEAD, fontSize: 13, bold: true, color: WHITE, charSpacing: 2.5 });
  txt(s, "Arya  ·  Gunoor  ·  Jahnvi  ·  Vikram", { x: 1.05, y: 5.75, w: 5.5, h: 0.32, fontSize: 14, color: G2 });
  txt(s, "UC Berkeley  ·  November 2026", { x: 1.05, y: 6.15, w: 5.5, h: 0.28, fontSize: 9.5, color: G5 });
  const slots = cols(2, 7.9, 4.93);
  phoneIn(s, "s-lock.jpg", slots[0].x, slots[0].w, 0.9, 5.0, null);
  phoneIn(s, "b-polist.jpg", slots[1].x, slots[1].w, 0.9, 5.0, null);
  label(s, "SELLER PROMPT", slots[0].x, 6.12, slots[0].w, G4, "center");
  label(s, "BUYER VIEW", slots[1].x, 6.12, slots[1].w, G4, "center");
  txt(s, "Live prototype: zara-reloop.vercel.app", { x: 7.9, y: 6.6, w: 4.93, h: 0.25, fontSize: 9.5, color: G5, align: "center" });
}

/* ========================= 1. PROBLEM ========================= */
{
  const s = pres.addSlide();
  frame(s, 0, "01  TARGET BEHAVIOUR", "Zara clothes are resold at scale, just not on Zara: its own resale sits outside the shopping journey, so buying new stays easier",
    "ThredUp Resale Report 2026; Vinted listing counts (Trendwatching, 2024); Scientific Reports 2025 (n=1,009 US); BCG x Vestiaire Collective 2022; YouGov US; ThredUp Resale Report 2025; Zara Pre-Owned US terms (Oct 2024); Zara iOS app, screenshots 14 Nov 2026");
  const aw = 3.55, bw = 4.98;
  const a = { x: M, w: aw }, b = { x: M + aw + GUT, w: bw }, c = { x: M + aw + bw + 2 * GUT, w: CW - aw - bw - 2 * GUT };
  const h = MAIN_BOTTOM - TOP;
  let i = card(s, a.x, TOP, a.w, h, "Decision moments");
  label(s, "THE BUYER", i.x, i.y, i.w);
  body(s, "On a product page, **their size is sold out** or the price feels high. Today Zara offers only \"Notify me\" or another new item.", { x: i.x, y: i.y + 0.3, w: i.w, h: 1.1 });
  s.addShape(pres.shapes.LINE, { x: i.x, y: i.y + 1.5, w: i.w, h: 0, line: { color: G3, width: 0.75 } });
  label(s, "THE OWNER", i.x, i.y + 1.65, i.w);
  body(s, "A Zara piece **has stopped being worn**. Listing it takes effort and the payoff is unclear, so it stays in the closet or is donated.", { x: i.x, y: i.y + 1.95, w: i.w, h: 1.1 });
  s.addShape(pres.shapes.LINE, { x: i.x, y: i.y + 3.15, w: i.w, h: 0, line: { color: G3, width: 0.75 } });
  body(s, "Goal: make **choosing and supplying pre-owned** as easy as buying new.", { x: i.x, y: i.y + 3.3, w: i.w, h: 0.6, fontSize: 10.5, color: G8 });

  i = card(s, b.x, TOP, b.w, h, "Three barriers");
  const rows = [
    ["1  HIDDEN", "Pre-Owned opens as a separate web page from item 07 of the menu. Product pages never show a pre-owned unit, even for a sold-out size.", "46%", "of shoppers check resale before buying new"],
    ["2  UNTRUSTED", "Condition is self-declared and the price is typed by the seller. The app suggests $7-13; the seller can still list at $50.", "45%", "of US adults worry about the condition of used clothes"],
    ["3  EFFORTFUL", "The owner must find, photograph, price, pack and post the item, for no reward, even when it has just sold out.", "41.7%", "of US consumers have never sold a garment"],
  ];
  const rowH = (i.h + 0.05) / 3, statW = 1.45, textW = i.w - statW - 0.2;
  rows.forEach((r, k) => {
    const y = i.y + k * rowH;
    label(s, r[0], i.x, y, textW, BLACK);
    body(s, r[1], { x: i.x, y: y + 0.28, w: textW, h: rowH - 0.4, fontSize: 10 });
    s.addShape(pres.shapes.RECTANGLE, { x: i.x + textW + 0.2, y, w: statW, h: rowH - 0.18, fill: { color: G1 }, line: { color: G2 } });
    txt(s, r[2], { x: i.x + textW + 0.3, y: y + 0.08, w: statW - 0.2, h: 0.42, fontFace: HEAD, fontSize: 20, bold: true, color: BLACK, lineSpacingMultiple: 1.0 });
    txt(s, stop(r[3]), { x: i.x + textW + 0.3, y: y + 0.52, w: statW - 0.2, h: rowH - 0.75, fontSize: 8, color: G8 });
    if (k < 2) s.addShape(pres.shapes.LINE, { x: i.x, y: y + rowH - 0.09, w: i.w, h: 0, line: { color: G2, width: 0.75 } });
  });

  i = card(s, c.x, TOP, c.w, h, "What a customer sees", { fill: G1 });
  const slots = cols(2, i.x, i.w), ph = 2.55;
  phoneIn(s, "appdark.jpg", slots[0].x, slots[0].w, i.y + 0.05, ph, "Native app (dark)", REAL);
  phoneIn(s, "web.jpg", slots[1].x, slots[1].w, i.y + 0.05, ph, "Pre-Owned (web view)", REAL);
  body(s, "Zara is the **#2 most-shopped brand** on ThredUp (orders +25% a year) and has **~62M items** listed on Vinted.", { x: i.x, y: i.y + ph + 0.6, w: i.w, h: 0.75, fontSize: 9, color: G8 });
  takeaway(s, "The demand for second-hand Zara exists. The barriers are **placement, trust and effort**, and all three sit inside Zara's own app and stores.");
}

/* ========================= 2. SOLUTION ========================= */
{
  const s = pres.addSlide();
  frame(s, 1, "02  THE CONCEPT", "ReLoop puts pre-owned one tap from new: Zara prompts owners with a price, AI grades the photos, and staff verify the garment once it sells",
    "Team prototype (zara-reloop.vercel.app), built on Zara's existing iOS app design and Zara Pre-Owned terms");
  const lanes = cols(2), laneH = 3.62;
  const lane = (L, title, shots) => {
    s.addShape(pres.shapes.RECTANGLE, { x: L.x, y: TOP, w: L.w, h: laneH, fill: { color: G1 }, line: { color: G2 } });
    label(s, title, L.x + 0.18, TOP + 0.14, L.w - 0.36, BLACK);
    cols(3, L.x + 0.1, L.w - 0.2).forEach((sl, k) => phoneIn(s, shots[k][0], sl.x, sl.w, TOP + 0.5, 2.5, shots[k][1]));
  };
  lane(lanes[0], "OWNER  ·  SELL IN ABOUT A MINUTE", [["s-lock.jpg", "1  Push: \"yours could sell for up to $82\""], ["s-offer.jpg", "2  Four photos, AI grade, Zara sets the price"], ["s-fulfil.jpg", "3  Listed; drop at a store only once it sells"]]);
  lane(lanes[1], "BUYER  ·  SAME PAGE, SAME BAG AS NEW", [["b-pdp.jpg", "1  SHOP PRE-OWNED under ADD TO BAG"], ["b-size.jpg", "2  Sold-out size shows PRE-OWNED IN M"], ["b-done.jpg", "3  Order turns ZARA VERIFIED before it ships"]]);
  const pil = [
    ["Accessible", "Native in the Zara app: on every product page, in the size picker and in the one bag. No separate web page."],
    ["Effortless", "Listing starts from the purchase record, so Zara already knows the item, size and original price. Four photos, one tap."],
    ["Trusted", "AI-GRADED at listing, so it goes live instantly. After a sale, staff check it in a few minutes and it becomes ZARA VERIFIED."],
    ["Rewarding", "The seller keeps 100% of a Zara-set price, +10% as Zara credit, plus a $0-10 bonus only when the item is sold out."],
  ];
  const py = TOP + laneH + 0.15, ph = TK_Y + TK_H - py;
  cols(4).forEach((c, k) => {
    s.addShape(pres.shapes.RECTANGLE, { x: c.x, y: py, w: c.w, h: ph, fill: { color: WHITE }, line: { color: G3, width: 0.75 }, shadow: shadow() });
    txt(s, pil[k][0], { x: c.x + 0.18, y: py + 0.12, w: c.w - 0.36, h: 0.32, fontFace: HEAD, fontSize: 14, bold: true, color: BLACK, lineSpacingMultiple: 1.0 });
    body(s, pil[k][1], { x: c.x + 0.18, y: py + 0.48, w: c.w - 0.36, h: ph - 0.6, fontSize: 9.5 });
  });
}

/* ========================= 3. INSIGHTS ========================= */
{
  const s = pres.addSlide();
  frame(s, 2, "03  CROSS-INDUSTRY INSIGHTS", "Each part of ReLoop has already changed behaviour elsewhere; Zara can combine them because it owns the purchase record, the app and the stores",
    "Patagonia press release (14 Oct 2026) and Trellis; Zalando release (28 May 2026); Vestiaire Collective Trust page and fee FAQ; ThredUp FY2025 10-K and seller terms; H&M Group annual report 2025; Sellpy FY2025 results; Angrist, Caldwell & Hall, AEJ: Applied (2021); US GAO-20-191 (2020)");
  const rows = [
    ["Lever", "Who proved it", "What happened", "What ReLoop takes for Zara"],
    ["Put used next to new", "Patagonia, Zalando", "Patagonia moved used items into its main store with one cart (Oct 2026): ~160k items, ~$13M a year. At Zalando, 40% of pre-owned buyers add a new item to the same order.", "SHOP PRE-OWNED on every product page and one bag, inside the native app."],
    ["Screen cheaply; check physically only when needed", "Vestiaire Collective, The RealReal", "Vestiaire's digital screen removes ~10% of listings, mostly for photo quality and condition. A physical check costs the buyer $15, half the price of a Zara garment.", "AI photo gate at listing; a 7-minute store check only after the item has sold."],
    ["Let the platform own the price", "ThredUp, Mercari, Depop", "ThredUp prices every item itself; Mercari steps prices down until they sell. ThredUp pays a seller only 15-30% on a $20-50 item.", "Zara sets the price from its own and market comparables; the seller keeps 100%."],
    ["Pay for supply only where it is scarce", "Uber, US airlines", "A 10% temporary pay rise brought ~12% more driver hours. Airlines raise the offer until enough passengers give up a seat: 43 volunteer for every 1 bumped.", "A Zara-credit bonus that rises only when an item is sold out and has a waitlist."],
    ["Avoid the warehouse trap", "H&M / Sellpy, ThredUp", "Sellpy lost SEK 165M on SEK 2.0bn of sales in 2025. ThredUp spends ~49% of revenue on operations for a 4% margin.", "Sell first, move later: the garment stays with the owner until it is bought."],
  ];
  const rh = (MAIN_BOTTOM - TOP - 0.4) / 5;
  table(s, rows, M, TOP, CW, [2.35, 1.8, 5.0, CW - 9.15], { fs: 10, rowH: [0.4, rh, rh, rh, rh, rh], boldFirst: true });
  takeaway(s, "No single company has all of these. **Only Zara knows what each customer owns, has the app they already open, and a store near them.**");
}

/* ========================= 4. ECONOMICS ========================= */
{
  const s = pres.addSlide();
  frame(s, 3, "04  IMPLEMENTATION & ECONOMICS", "Through Zara's stores, ReLoop serves a garment 40% cheaper than warehouse resale and pays for itself above ~0.4% of US sales",
    "Team financial model (US, 3 years; appendix A6-A7). Zara Pre-Owned US terms; USPS Notice 123 (Oct 2026); BLS ECEC (Dec 2025); Zara US job postings; Stripe Connect; Inditex FY2025; H&M Group 2025; Patagonia 2026. Inditex does not disclose US sales, so the base is an estimate");
  const [c1, c2, c3] = cols(3), h = MAIN_BOTTOM - TOP;
  let i = card(s, c1.x, TOP, c1.w, h, "How it runs");
  const steps = [["LISTED", "AI-graded, live in the app"], ["SOLD", "Buyer pays in one checkout"], ["DROP-OFF", "Seller brings it to a store"], ["ZARA VERIFIED", "7-minute staff check, ships"]];
  steps.forEach((st, k) => {
    const y = i.y + k * 0.46;
    s.addShape(pres.shapes.OVAL, { x: i.x, y, w: 0.32, h: 0.32, fill: { color: k === 3 ? BLACK : G2 }, line: { color: k === 3 ? BLACK : G4 } });
    txt(s, String(k + 1), { x: i.x, y, w: 0.32, h: 0.32, fontSize: 10, bold: true, color: k === 3 ? WHITE : BLACK, align: "center", valign: "middle" });
    txt(s, [{ text: st[0] + "  ", options: { bold: true, color: BLACK, fontSize: 10 } }, { text: st[1], options: { color: G8, fontSize: 9.5 } }], { x: i.x + 0.45, y, w: i.w - 0.45, h: 0.32, valign: "middle" });
  });
  s.addShape(pres.shapes.LINE, { x: i.x, y: i.y + 1.95, w: i.w, h: 0, line: { color: G2, width: 0.75 } });
  label(s, "ALREADY EXISTS", i.x, i.y + 2.08, i.w);
  body(s, "Pre-Owned platform, Stripe payouts, prepaid labels, store returns desks and ship-from-store.", { x: i.x, y: i.y + 2.34, w: i.w, h: 0.55, fontSize: 9.5 });
  label(s, "ZARA MUST BUILD  ·  $4.8M ONE-OFF", i.x, i.y + 2.98, i.w);
  body(s, "Native app module, pricing and AI-grading services, a store verify screen and kit, and staff training. No warehouse.", { x: i.x, y: i.y + 3.24, w: i.w, h: 0.55, fontSize: 9.5 });

  i = card(s, c2.x, TOP, c2.w, h, "Per garment sold, Year 3 ($)");
  s.addChart(pres.charts.BAR, [{ name: "Per garment", labels: ["Fees less cost to serve", "Credit spent on new items", "Saved and bigger baskets", "New-to-Zara customers", "Seller bonuses (credit)", "New sales replaced", "Net per garment"],
    values: [-2.19, 5.99, 4.22, 2.50, -2.80, -5.57, 2.15] }], {
    x: i.x - 0.1, y: i.y - 0.05, w: i.w + 0.2, h: 2.95, barDir: "bar", chartColors: [BLACK], invertedColors: [G4], catAxisOrientation: "maxMin", catAxisLabelPos: "low",
    showValue: true, dataLabelFormatCode: "+0.00;-0.00", dataLabelFontSize: 8.5, dataLabelColor: INK, dataLabelPosition: "outEnd", dataLabelFontFace: BODY,
    catAxisLabelFontSize: 8.5, catAxisLabelColor: G9, catAxisLabelFontFace: BODY, valAxisHidden: true, valGridLine: { style: "none" }, catGridLine: { style: "none" },
    catAxisLineShow: false, showLegend: false, barGapWidthPct: 45, valAxisMinVal: -8, valAxisMaxVal: 8,
  });
  s.addShape(pres.shapes.LINE, { x: i.x, y: i.y + 3.0, w: i.w, h: 0, line: { color: G2, width: 0.75 } });
  body(s, "Cost to serve **$12.70** vs **$21.14** in a warehouse model, which pays an inbound parcel, storage and ~30% rejects.", { x: i.x, y: i.y + 3.12, w: i.w, h: 0.62, fontSize: 9.5 });

  i = card(s, c3.x, TOP, c3.w, h, "Three scenarios, Year 3 (US)");
  const sc = [["", "Typical", "Base", "Upside"], ["Benchmark", "<0.1% of sales", "~0.16%", "~0.7% (H&M)"], ["Garments resold", "31k", "246k", "873k"], ["Net new revenue", "$0.3M", "$6.7M", "$27.0M"], ["Operating result", "-$1.7M", "-$0.7M", "+$3.0M"], ["Cash after 3 yrs", "-$9.8M", "-$6.8M", "+$0.3M"]];
  table(s, sc, i.x, i.y - 0.02, i.w, [1.12, 0.84, 0.72, i.w - 2.68], { fs: 8.5, rowH: 0.34, boldFirst: true });
  body(s, "Break-even at **~560k garments a year**. Net new revenue already subtracts the new sales that pre-owned replaces.", { x: i.x, y: i.y + 2.2, w: i.w, h: 0.62, fontSize: 9.5, color: G8 });
  body(s, "Zara says today's Pre-Owned is not yet profitable. ReLoop costs **~$7M over 3 years**, under 0.03% of one year of Inditex capex.", { x: i.x, y: i.y + 2.92, w: i.w, h: 0.8, fontSize: 9.5, color: G8 });
  takeaway(s, "Profit is not the goal, but the cost is bounded: **every sale covers its own cost**, and scale decides whether the programme funds itself.");
}

/* ========================= 5. PILOT ========================= */
{
  const s = pres.addSlide();
  frame(s, 4, "05  PILOT & SUCCESS METRICS", "A six-month, two-city pilot with a hold-out group proves whether ReLoop gets more garments re-worn before Zara scales it",
    "Team pilot design. Benchmarks: ThredUp seller terms (60-80% acceptance); Zalando 2026 (62% of pre-owned sells within 7 days); push and back-in-stock benchmarks (Airship, Batch, Omnisend 2025)");
  const L = { x: M, w: 7.45 }, R = { x: M + 7.45 + GUT, w: CW - 7.45 - GUT }, gH = 2.45;
  let i = card(s, L.x, TOP, L.w, gH, "Timeline and decision gates");
  const gx = i.x + 2.2, cw = (i.w - 2.2) / 6;
  ["M1", "M2", "M3", "M4", "M5", "M6"].forEach((m, k) => txt(s, m, { x: gx + k * cw, y: i.y - 0.06, w: cw, h: 0.2, fontSize: 8.5, bold: true, color: G6, align: "center" }));
  const bars = [["Build and label", "Rubric, verify screen", 0, 1, G4], ["Seller side live", "NYC + LA, past season", 1, 2, G8], ["Buyer side + sold out", "A/B on product pages", 3, 2, BLACK], ["Decide", "Scale, fix or stop", 5, 1, G6]];
  bars.forEach((b, k) => {
    const y = i.y + 0.2 + k * 0.34;
    txt(s, [{ text: b[0], options: { bold: true, fontSize: 9.5, color: BLACK, breakLine: true } }, { text: b[1], options: { fontSize: 8, color: G6 } }], { x: i.x, y: y - 0.04, w: 2.15, h: 0.36, valign: "middle", lineSpacingMultiple: 1.0 });
    s.addShape(pres.shapes.RECTANGLE, { x: gx + b[2] * cw + 0.04, y: y + 0.04, w: b[3] * cw - 0.08, h: 0.22, fill: { color: b[4] }, line: { color: b[4] } });
  });
  body(s, "**Gates:** M3 listing ≥ 1% per prompt, staff ≤ 8 min  ·  M5 sell-through ≥ 50%, disputes < 5%  ·  M6 no drop in full-price sales", { x: i.x, y: i.y + 1.6, w: i.w, h: 0.4, fontSize: 8.5, color: G8 });
  const rY = TOP + gH + 0.15;
  i = card(s, L.x, rY, L.w, MAIN_BOTTOM - rY, "Top risks and how the pilot contains them");
  bullets(s, [
    "**Replaces new sales:** start with past-season and sold-out items, test against a 10% hold-out",
    "**Credit only buys new clothes:** track credit spent on pre-owned, and listing after the bonus ends",
    "**Store queues:** QR drop-off at the returns desk, checks in quiet hours, mail-in as overflow",
  ], i.x, i.y, i.w, i.h, 10.5, 9);
  i = card(s, R.x, TOP, R.w, MAIN_BOTTOM - TOP, "Success = more garments in use");
  const k6 = [["Garments re-worn", "Sold and Zara-verified units", "47k Y1 · 246k Y3"], ["Owners who list", "Prompted owners listing at least one item", "≥ 1.5% per prompt"], ["Sold-out rescues", "Sold-out sessions ending in a pre-owned buy", "Up vs hold-out"],
    ["Habit without a bonus", "Sellers who list again with no bonus", "≥ 30% in 90 days"], ["Trust", "Buyer disputes after ZARA VERIFIED", "< 4%"], ["Store load", "Staff minutes per verified item", "≤ 7 min"]];
  const rh = (i.h + 0.05) / 6, boxW = 1.5;
  k6.forEach((r, k) => {
    const y = i.y + k * rh;
    txt(s, [{ text: r[0], options: { bold: true, fontSize: 10, color: BLACK, breakLine: true } }, { text: r[1], options: { fontSize: 8.5, color: G6 } }], { x: i.x, y, w: i.w - boxW - 0.15, h: rh - 0.1, valign: "middle", lineSpacingMultiple: 1.0 });
    s.addShape(pres.shapes.RECTANGLE, { x: i.x + i.w - boxW, y: y + 0.03, w: boxW, h: rh - 0.16, fill: { color: G1 }, line: { color: G2 } });
    txt(s, r[2], { x: i.x + i.w - boxW, y: y + 0.03, w: boxW, h: rh - 0.16, fontSize: 9.5, bold: true, color: BLACK, align: "center", valign: "middle" });
  });
  takeaway(s, "Zara learns in **six months, in two cities, with written stop rules** whether integration plus verification changes what customers do, and only then scales it.");
}

/* ========================= APPENDIX ========================= */
{
  const s = pres.addSlide();
  s.background = { color: BLACK };
  s.addShape(pres.shapes.RECTANGLE, { x: 9.4, y: 0, w: W - 9.4, h: H, fill: { color: "161616" }, line: { color: "161616" } });
  s.addImage({ path: D + "logo_white.png", x: 0.8, y: 0.8, w: 1.4, h: 1.4 * LOGO });
  txt(s, "Appendix", { x: 0.8, y: 2.8, w: 11, h: 1.0, fontFace: HEAD, fontSize: 48, color: WHITE, lineSpacingMultiple: 1.0 });
  txt(s, "Supporting material: user journeys, algorithms, store operations, financial model, case-study detail, risks and references.", { x: 0.8, y: 3.95, w: 7.8, h: 0.9, fontSize: 14, color: G4 });
  page += 1;
}
const apx = (k, t, src) => { const s = pres.addSlide(); frame(s, -1, k, t, src, true); return s; };

{
  const s = apx("A1  USER JOURNEYS", "End to end, the seller takes four photos and makes one drop-off; the buyer never leaves the normal Zara flow", "Team prototype, zara-reloop.vercel.app (buyer and seller flows)");
  const sh = [["s-offer-intro.jpg", "Seller sees what it could sell for by grade, plus the live demand bonus"], ["s-offer.jpg", "AI grade, Zara price and comparable range; accept, appeal or decline"], ["s-sold.jpg", "\"Sold\" push: drop it at the chosen store by the deadline"],
    ["s-next.jpg", "Next steps and the staff check that makes it ZARA VERIFIED"], ["b-polist.jpg", "Buyer sees every graded unit of this item, with owners' photos"], ["b-listing.jpg", "Listing: the owner's photo next to the original, AI-GRADED"]];
  cols(6).forEach((c, k) => phoneIn(s, sh[k][0], c.x, c.w, TOP + 0.05, 3.55, sh[k][1]));
  takeaway(s, "Everything a customer touches already looks and behaves like the Zara app. **The only new habits are \"list it\" and \"drop it off\".**", 6.04);
}

{
  const s = apx("A2  TWO-STAGE TRUST", "Listings go live as AI-GRADED; Zara only handles a garment once it has sold, and nothing ships until staff mark it ZARA VERIFIED",
    "ISO 12945-2 pilling scale; Sensors 2023 (garment defect detection, mAP 0.6-0.9); Furferi 2014 (human grader agreement under 80%); Vestiaire Collective Trust page; The RealReal Q3 2026 results");
  const [l, r] = cols(2), topH = 2.35, bY = TOP + topH + GUT, bH = APX_BOTTOM - bY;
  let i = card(s, l.x, TOP, l.w, topH, "Stage 1  ·  AI-GRADED at listing");
  bullets(s, ["Four coached photos: front, back, care label and highest-wear point; the shutter unlocks only when light, sharpness and framing pass",
    "A vision model scores pilling (ISO 1-5), stains, holes, fading and hardware, mapped to Zara's existing ladder",
    "The listing goes live immediately with the grade, the evidence and the owner's photos. **Zara has not touched the garment**"], i.x, i.y, i.w, i.h, 10.5, 7);
  i = card(s, r.x, TOP, r.w, topH, "Stage 2  ·  ZARA VERIFIED after purchase");
  bullets(s, ["The seller drops the sold garment at a store or mails it; the associate scans a QR code and the handheld shows the listing and a five-point rubric",
    "If it matches, the label prints, the order turns **ZARA VERIFIED** and it ships with the day's online orders",
    "If not, the buyer is offered the corrected price or a full refund, and the seller the lower payout or the garment back"], i.x, i.y, i.w, i.h, 10.5, 7);
  const tiers = [["Grade", "Definition", "Price vs original*"], ["NEW WITH TAGS", "Tags attached, no findings", "~65%"], ["LIKE NEW", "No visible findings, pilling 5", "~50%"], ["GOOD", "Minor findings only, pilling 4", "~38%"], ["FAIR", "Visible wear, pilling 3, one small mark", "~28%"], ["Below FAIR", "Routed to Zara Repair or Donate", "–"]];
  table(s, tiers, l.x, bY, l.w, [1.55, l.w - 3.05, 1.5], { fs: 10, rowH: (bH - 0.45) / 6, boldFirst: true });
  txt(s, "*Before the scarcity multiplier, for jackets, coats, dresses and knitwear. Benchmark: 2.77M Vinted sales (very good 50%, good 36% of the tagged-new price).", { x: l.x, y: APX_BOTTOM - 0.36, w: l.w, h: 0.36, fontSize: 7.5, color: G6, valign: "bottom" });
  i = card(s, r.x, bY, r.w, bH, "Why both stages");
  bullets(s, ["Lab pilling classifiers reach 94-99%, but on real photos stain and hole detection is only 0.6-0.9 mAP, and trained humans agree under 80% of the time",
    "So the AI sets expectations and turns the store check into a few minutes, not a full inspection. **Every staff decision becomes a training label**",
    "Vestiaire removes ~10% of listings at its digital screen, mostly for photos and condition, the same problem Zara has"], i.x, i.y, i.w, i.h, 10.5, 7);
}

{
  const s = apx("A3  PRICING ENGINE", "Zara, not the seller, sets one price per item from its own sales and market comparables, then shows what the seller would keep elsewhere",
    "eBay Developer Program (Browse API limits; Marketplace Insights); Depop Partner API (Pricing Inspiration); Bright Data dataset pricing; Mercari Engineering (Smart Pricing); Kaggle Mercari challenge (RMSLE 0.39); 2026 marketplace fee pages");
  const leftW = 7.95, R = { x: M + leftW + GUT, w: CW - leftW - GUT };
  const steps = [["0  Identity", "From the order record: reference (e.g. 2757/397/403), size, original price and season."],
    ["1  Comparables", "Zara Pre-Owned's own sales first, then eBay and Depop APIs, licensed Poshmark data and a ThredUp feed."],
    ["2  Normalise", "Convert asks to expected sold prices, map each source to Zara's four grades, and decay older comps."],
    ["3  Model fallback", "When comps are thin: original price x grade x age x season; later a model trained on Zara's sales."],
    ["4  Scarcity", "x1.0 to x1.5 when sold out with a waitlist; never above 85% of original or the live new price."],
    ["5  List and step down", "List at the price expected to clear in 14 days; -7% at days 10 and 20; the seller's floor holds."]];
  const two = cols(2, M, leftW), boxH = 1.16;
  steps.forEach((st, k) => {
    const c = two[k % 2], y = TOP + Math.floor(k / 2) * (boxH + GUT);
    const i = card(s, c.x, y, c.w, boxH, null);
    txt(s, st[0], { x: i.x, y: y + 0.12, w: i.w, h: 0.3, fontFace: HEAD, fontSize: 13, bold: true, color: BLACK, lineSpacingMultiple: 1.0 });
    body(s, st[1], { x: i.x, y: y + 0.46, w: i.w, h: boxH - 0.55, fontSize: 9.5 });
  });
  const wy = TOP + 3 * (boxH + GUT);
  s.addShape(pres.shapes.RECTANGLE, { x: M, y: wy, w: leftW, h: APX_BOTTOM - wy, fill: { color: G1 }, line: { color: G2 } });
  body(s, "**Worked example (the prototype blazer).** Original $109, sold out in M with a deep waitlist, so x1.5. Like new: 109 x 0.50 x 1.5 = **$82**. Good: x 0.385 = **$63**. Fair: x 0.28 = **$46**. Expected error at launch is ±25-35%, so the seller sees a band ($38-92), not a single point.",
    { x: M + 0.18, y: wy, w: leftW - 0.36, h: APX_BOTTOM - wy, fontSize: 9.5, valign: "middle" });
  const i = card(s, R.x, TOP, R.w, APX_BOTTOM - TOP, "Seller keeps on a $63 sale");
  s.addChart(pres.charts.BAR, [{ name: "Kept", labels: ["ReLoop, Zara credit", "ReLoop, cash", "Depop", "Mercari", "eBay", "Poshmark", "ThredUp consignment"], values: [69.3, 63.0, 60.47, 56.7, 54.03, 50.4, 26.85] }], {
    x: i.x - 0.1, y: i.y - 0.05, w: i.w + 0.2, h: 3.6, barDir: "bar", chartColors: [BLACK, G8, G4, G4, G4, G4, G4], catAxisOrientation: "maxMin", showValue: true, dataLabelFormatCode: "$0.00", dataLabelFontSize: 8.5,
    dataLabelColor: INK, dataLabelPosition: "outEnd", dataLabelFontFace: BODY, catAxisLabelFontSize: 8.5, catAxisLabelColor: G9, catAxisLabelFontFace: BODY, valAxisHidden: true, valGridLine: { style: "none" }, catGridLine: { style: "none" }, showLegend: false, valAxisMaxVal: 85, barGapWidthPct: 40,
  });
  body(s, "Same sale price on every channel; fee rules from 2026 fee pages. On every other channel the seller also photographs, prices and ships.", { x: i.x, y: i.y + 3.65, w: i.w, h: 0.6, fontSize: 8.5, color: G6 });
}

{
  const s = apx("A4  DYNAMIC DEMAND BONUS", "The bonus rises only when an item is scarce and is paid only when it sells; a flat bonus would cost more than the supply it brings",
    "Angrist, Caldwell & Hall, AEJ: Applied (2021); DoorDash Peak Pay; DICE Waiting List; Omnisend 2025 back-in-stock benchmarks; brand trade-in pages (Lululemon, Levi's, Arc'teryx, Patagonia); Inditex FY2025 gross margin 58.3%");
  const [l, r] = cols(2), topH = 2.55, bY = TOP + topH + GUT, bH = APX_BOTTOM - bY;
  let i = card(s, l.x, TOP, l.w, topH, "How it is set");
  bullets(s, ["**Scarcity** per item and size, hourly: expected 14-day demand (Notify-me sign-ups, sold-out page views, failed searches) divided by listed units + 1",
    "**Bonus** is $0 below 1, then rises on a log curve to a **$10 cap** at 8x; it averages about $5 on sold-out listings",
    "**Guardrails:** paid only if it sells within 30 days; full value if listed within 7 days, half to day 30; $20 per customer per month; weekly budget"], i.x, i.y, i.w, i.h, 10.5, 7);
  i = card(s, r.x, TOP, r.w, topH, "Why not a flat $10");
  bullets(s, ["Measured supply response to a temporary pay rise: **+10% pay brings ~12% more supply** (Uber drivers, randomised)",
    "A flat $5 on a $30 payout is +17%, so ~20% more listings. Paid on every sold-out item, that is **~$22 per extra garment vs ~$18 of value**",
    "Targeted at deep waitlists, the same budget buys supply where each garment is worth most. No apparel brand does this today"], i.x, i.y, i.w, i.h, 10.5, 7);
  const bt = [["Scarcity", "0.5", "1", "2", "3", "5", "8+"], ["Bonus (Zara credit)", "$0", "$3", "$5", "$6", "$8", "$10"], ["Cost to Zara*", "$0", "$2.19", "$3.64", "$4.37", "$5.83", "$7.28"]];
  const cw6 = (l.w - 1.9) / 6;
  table(s, bt, l.x, bY, l.w, [1.9, cw6, cw6, cw6, cw6, cw6, cw6], { fs: 10, rowH: 0.42, boldFirst: true, right: true });
  txt(s, "*$1 of credit costs 42c if it funds a purchase that would not have happened (1 − 58.3% gross margin) and $1 if it would have; blended 73c at 40% incrementality.", { x: l.x, y: APX_BOTTOM - 0.4, w: l.w, h: 0.4, fontSize: 8, color: G6, valign: "bottom" });
  i = card(s, r.x, bY, r.w, bH, "What brands pay today (all flat)");
  bullets(s, ["Lululemon $5 / $10 / $25 by category; Levi's $5-30; Madewell $20; The North Face up to $90",
    "Arc'teryx 30% of original price; Patagonia about 20%",
    "Credit top-ups: The RealReal +5%, Fashionphile +10%"], i.x, i.y, i.w, i.h, 12, 10);
}

{
  const s = apx("A5  STORE OPERATIONS", "At Year-3 volume a store handles about seven items a day at a desk that already processes returns and ships online orders",
    "Team model; Zara US pay $18.21-18.50/h (ZipRecruiter, Inditex Careers postings, 2026) x 1.30 BLS benefits load; Inditex and Retail Dive on ship-from-store; PayPal/TRC 2022 (54% prefer in-person drop-off)");
  const st = [["7 min", "staff time per item: 4 to verify, 3 to pack and label"], ["~7 a day", "items per US store at Year-3 base volume (24 in the upside)"], ["< 1 hour", "staff time per store per day; ~14 FTE across 102 stores"], ["$2.80", "labour per item at $0.40 per loaded minute"]];
  const sh = 1.45;
  cols(4).forEach((c, k) => { card(s, c.x, TOP, c.w, sh, null, { fill: k === 0 ? BLACK : WHITE }); stat(s, c.x + 0.2, TOP + 0.18, c.w - 0.4, st[k][0], st[k][1], k === 0); });
  const [l, r] = cols(2), y = TOP + sh + GUT, h = APX_BOTTOM - y;
  let i = card(s, l.x, y, l.w, h, "Service levels");
  bullets(s, ["The seller has **5 days** to drop off after a sale (reminders on days 2 and 4); on day 5 the order cancels and the buyer is offered the next-best unit",
    "The store ships within **24 hours** of drop-off, with the day's ship-from-store parcels",
    "Mail-in uses a prepaid label ($4.95 deducted, as for Zara returns) to two verification points inside existing facilities",
    "Repeat no-shows lose bonus eligibility"], i.x, i.y, i.w, i.h, 12, 12);
  i = card(s, r.x, y, r.w, h, "Store kit and training");
  bullets(s, ["One bin and one small inspection station at the returns desk ($2,500 per store)",
    "A \"verify\" screen on the existing handheld: scan the QR code, compare with the listing photos, tick five checks, print the label",
    "Two hours of training per associate, with content reused across markets",
    "A daily drop-off cap per store in the app, and verification in quiet hours on peak days"], i.x, i.y, i.w, i.h, 12, 12);
}

{
  const s = apx("A6  UNIT ECONOMICS", "Each garment loses about $2 on fees and logistics; the loop around the sale (credit, baskets, new customers) more than pays for it",
    "Team model, base case, Year 3. Zara Pre-Owned US terms ($1 + 5%); USPS Notice 123; Stripe Connect; Capital One and First Data gift-card studies; Zalando 2026; INFORMS M&SOM 2023; WRAP 2025");
  const rows = [["Line ($ per garment sold, $30 average)", "Value", "Basis"],
    ["Buyer fee ($1 + 10% after the pilot; $1 + 5% during it)", "+4.00", "Zara's fee today is $1 + 5%; peers charge 10-20%"],
    ["Shipping charged at cost", "+6.50", "As Pre-Owned does today"],
    ["Mail-in deduction (half of sellers)", "+2.48", "$4.95, the same rule as Zara returns"],
    ["Outbound and inbound parcels", "-9.75", "$6.50 per leg; half arrive by hand"],
    ["Store labour, packaging, payments, AI, exceptions, service", "-5.41", "7 min at $0.40; Stripe; 4% exceptions"],
    ["Transaction contribution", "-2.19", ""],
    ["Margin on new items bought with Zara credit", "+5.99", "70% take credit; 40% incremental (assumption)"],
    ["Saved baskets and new add-ons", "+4.22", "Zalando: 40% add a new item; we count a quarter"],
    ["New-to-Zara customers buying new", "+2.50", "20% new to brand; 25% buy within a year"],
    ["Credit top-up and demand bonus", "-2.80", "$3.85 face value at 73c per $1"],
    ["New sales replaced (20% blended)", "-5.57", "25% sold out / 60% in season / 10% past season"],
    ["Net contribution per garment", "+2.15", "+0.68 in the pilot year"]];
  table(s, rows, M, TOP, CW, [5.6, 1.2, CW - 6.8], { fs: 10, rowH: (APX_BOTTOM - TOP) / 13, boldRow: 12 });
}

{
  const s = apx("A7  FINANCIAL MODEL", "Base case: 246k garments a year by Year 3 and $6.7M of net new revenue, about $0.7M a year short of covering its team",
    "Team model (live spreadsheet shared separately). US sales are not disclosed: range $3-7bn, base $4.5bn (below Spain's EUR 6.3bn; Americas EUR 7.1bn in FY2025; ECDB models US online at $1.45bn)");
  const tw = 7.6, R = { x: M + tw + GUT, w: CW - tw - GUT };
  const rows = [["$M unless stated (base case)", "Year 0", "Year 1", "Year 2", "Year 3", "Total"],
    ["Garments sold (thousand)", "", "47", "211", "246", "504"], ["Paid to sellers", "", "1.4", "6.3", "7.4", "15.1"], ["Platform revenue (fees, shipping)", "", "0.5", "2.7", "3.2", "6.5"],
    ["Extra new-item sales", "", "1.2", "5.4", "6.3", "12.8"], ["New sales replaced", "", "-0.5", "-2.4", "-2.7", "-5.6"], ["Net new revenue", "", "1.2", "5.8", "6.7", "13.7"],
    ["Variable contribution", "", "0.0", "0.5", "0.5", "1.0"], ["Team, data, cloud", "", "-0.8", "-1.1", "-1.2", "-3.1"], ["Operating result", "", "-0.8", "-0.6", "-0.7", "-2.1"], ["Capex", "-4.8", "", "", "", "-4.8"]];
  table(s, rows, M, TOP, tw, [3.1, 0.9, 0.9, 0.9, 0.9, 0.9], { fs: 10, rowH: (APX_BOTTOM - TOP) / 11, boldFirst: true, right: true, boldRow: 6 });
  const i = card(s, R.x, TOP, R.w, APX_BOTTOM - TOP, "What it takes to pay for itself");
  bullets(s, ["Break-even at **~560k garments a year** (~0.4% of US sales); H&M (0.8%) and Patagonia (~0.9%) are above that after several years",
    "Volume is the lever. Prompt conversion at 3% instead of 1.5% takes Year 3 to about -$0.1M; half the cannibalisation or 55% credit incrementality takes it to break-even",
    "If today's policies stay (5% fee, $4.95 shipping, cash-first payout), the loss widens by **$1.1M a year**",
    "Europe's 16 other Pre-Owned markets share the same software at roughly six times US sales"], i.x, i.y, i.w, i.h, 12, 12);
}

{
  const s = apx("A8  CASE STUDIES IN DETAIL", "Scaled to Zara's prices, the integration and pricing lessons transfer; the warehouse model and luxury-style authentication do not",
    "Patagonia press release and Work in Progress report; PopSci, GearJunkie, Trellis (Oct-Nov 2026); Zalando release 28 May 2026; Vestiaire FAQ and CEO to Bloomberg (Feb 2026); The RealReal Q3 2026 8-K; ThredUp Q3 2026 8-K, FY2025 10-K, 2021 S-1; H&M Group annual report 2025");
  const cs = [
    ["Patagonia Worn Wear", ["Used items moved into the main store with one cart on 14 Oct 2026", "~160k items, ~$13M, ~0.9% of revenue (FY2026)", "40+ staff in Reno process 230k items a year (~20 min each)", "Only about half of customers knew Worn Wear existed"], "For Zara: copy the placement, not the handling. 20 minutes is 27% of a $30 garment; ReLoop uses 7."],
    ["Zalando Pre-owned", ["Pre-owned sits inside the main shop and checkout", "40% of pre-owned buyers add a new item to the same order", "62% of the range sells within 7 days", "Sellers are paid in Zalando credit"], "For Zara: proof that a mixed bag works at mass-market prices."],
    ["Vestiaire & RealReal", ["Vestiaire: ~10% of listings removed at its digital screen (2024)", "The buyer pays $15 for physical authentication", "Vestiaire has banned Zara as fast fashion since Nov 2023", "The RealReal's AI intake: ~35% of units, target ~50% (2026)"], "For Zara: take the cheap digital gate; a $15 hub check is half the garment."],
    ["ThredUp", ["FY2025 revenue $310.8M (+19.5%), adjusted EBITDA 4.4%", "Operations, product and tech are 49% of revenue", "Seller payout 3-80% by price band; 60-80% of a bag accepted", "Zara is its #2 brand by orders (+25% a year)"], "For Zara: demand is proven; the warehouse cost is what to avoid."],
  ];
  const h = APX_BOTTOM - TOP;
  cols(4).forEach((c, k) => {
    const i = card(s, c.x, TOP, c.w, h, cs[k][0]);
    bullets(s, cs[k][1], i.x, i.y, i.w, 3.3, 11.5, 12);
    const by = TOP + h - 1.3;
    s.addShape(pres.shapes.RECTANGLE, { x: c.x + 0.12, y: by, w: c.w - 0.24, h: 1.15, fill: { color: G1 }, line: { color: G2 } });
    txt(s, cs[k][2], { x: c.x + 0.25, y: by, w: c.w - 0.5, h: 1.15, fontSize: 10.5, bold: true, color: BLACK, valign: "middle" });
  });
}

{
  const s = apx("A9  RISKS AND MITIGATIONS", "The two risks that decide the economics, cannibalisation and incentive spend, are the two the pilot measures against a hold-out",
    "Team analysis; Zara case FAQ (Q10 on incentives, Q9 on donation partners); INFORMS M&SOM 2023; WRAP 2025; Sift 2025 marketplace fraud benchmarks");
  const rows = [["Risk", "Why it matters", "Mitigation"],
    ["Pre-owned replaces new sales", "At 1.5x our assumption, Year 3 is $0.7M worse.", "Launch on past-season and sold-out items; rank pre-owned below ADD; cap prices at 85% of original; hold-out A/B; category kill switch."],
    ["Incentive mainly drives new purchases", "The goal is garments re-worn, not more new buying.", "Pay the bonus only on scarce items that sell; track the share of credit spent on pre-owned and repeat listing without a bonus."],
    ["Store queues and labour", "At 12 min an item, Year 3 is $0.5M worse.", "QR drop-off at the returns desk, verification in quiet hours, mail-in overflow, daily caps."],
    ["Grade disputes", "Trust is the product.", "The staff check is binding; corrected price or refund before shipping; disputes become training data."],
    ["Fraud and bonus gaming", "The bonus could be farmed.", "The item must be in the customer's own Zara purchase history and checked in person; $20 per month cap."],
    ["Too few listings or buyers early", "Cold start.", "Seed supply first (months 2-3); prompt owners of items with live waitlists; unsold items step down, then exit to Donate."],
    ["Donation partners", "Donated stock must not be diverted.", "ReLoop lists only items owners choose to sell; donation bins and partners are untouched."],
    ["Privacy", "\"Zara knows what I own.\"", "Prompts only inside the customer's account; one-tap opt-out; at most six prompts a year."]];
  table(s, rows, M, TOP, CW, [2.75, 3.1, CW - 5.85], { fs: 10, rowH: (APX_BOTTOM - TOP) / 9, boldFirst: true });
}

{
  const s = apx("A10  CONTEXT", "Zara has the scale, the stores and the customers; second-hand demand is growing faster than new, and regulation is moving the same way",
    "Inditex FY2025 and 1H2026 results; ThredUp Resale Report 2026 (GlobalData); Inditex sustainability report 2025; EU Waste Framework Directive revision (2025); France Law 2026-602; California SB 707; WRAP 2025");
  const st = [["€39.9bn", "Inditex sales FY2025; Zara segment €28.1bn at a 58.3% gross margin"], ["102", "Zara stores in the US, Inditex's second-largest market"], ["$55.5bn", "US second-hand apparel in 2025, heading to $78.8bn by 2030"], ["59%", "of US consumers bought second-hand in 2025"]];
  const sh = 1.45;
  cols(4).forEach((c, k) => { card(s, c.x, TOP, c.w, sh, null, { fill: k === 0 ? BLACK : WHITE }); stat(s, c.x + 0.2, TOP + 0.18, c.w - 0.4, st[k][0], st[k][1], k === 0); });
  const [l, r] = cols(2), y = TOP + sh + GUT, h = APX_BOTTOM - y;
  let i = card(s, l.x, y, l.w, h, "Zara Pre-Owned today");
  bullets(s, ["Live in 17 markets: the UK (Nov 2022), France, 14 EU markets and the US (Oct 2024)",
    "Peer to peer: the seller pays nothing; the buyer pays $1 + 5% plus shipping; the seller ships within 3 days; all sales are final",
    "Repair runs through specialist workshops; donation through collection bins and partners such as The Salvation Army",
    "Zara said at launch it did not expect early profitability; no usage figures are published"], i.x, i.y, i.w, i.h, 12, 12);
  i = card(s, r.x, y, r.w, h, "Why now");
  bullets(s, ["Inditex's Scope 3 emissions were flat 2018-2024 against a -51% target for 2030, and are over 99% of its footprint",
    "EU textile EPR must run in every member state by ~2028, with fees that can reward \"use beyond the first user\"",
    "France's anti-fast-fashion law (Jul 2026) sets per-item penalties rising to €2.20-19.50 by 2030; California SB 707 is in force",
    "Resale displaces ~65% of new purchases market-wide (WRAP), so each re-worn garment matters"], i.x, i.y, i.w, i.h, 12, 12);
}

{
  const s = apx("A11  METHODOLOGY", "How we built the case: a product teardown, a working prototype, desk research on 30+ sources and a scenario model", "Team work, November 2026");
  const m = [["Teardown", "Walked Zara's iOS app and Pre-Owned flows end to end (screenshots 14 Nov 2026): menu, product page, bag, listing flow, price hint and condition ladder."],
    ["Prototype", "Built both journeys as a clickable iPhone prototype in Zara's design language, with a working camera capture and live quality checks."],
    ["Desk research", "Company filings (Inditex, ThredUp, The RealReal, H&M), brand releases, academic studies and industry benchmarks, each tagged confirmed, estimate or assumption."],
    ["Model", "A three-year US model with live formulas: funnel, unit economics, capex, scenarios anchored on published resale penetration, and sensitivity on every assumption."]];
  const topH = 2.5;
  cols(4).forEach((c, k) => { const i = card(s, c.x, TOP, c.w, topH, m[k][0]); body(s, m[k][1], { x: i.x, y: i.y, w: i.w, h: i.h, fontSize: 12 }); });
  const y = TOP + topH + GUT, i = card(s, M, y, CW, APX_BOTTOM - y, "Limits we state openly");
  bullets(s, ["Inditex does not disclose US sales, units or customers; the US base is an estimate with a range",
    "No published benchmark exists for price-led \"you own this\" prompts, for how much resale credit is incremental, or for in-store verification of clothing under $60. These are exactly what the pilot measures",
    "Research used public sources only, and no survey was run in Round 1; figures marked as estimates or assumptions should be read that way"], i.x, i.y, i.w, i.h, 12, 12);
}

{
  const s = apx("A12  REFERENCES", "References", null);
  const refs = [
    "Inditex, FY2025 results (11 Mar 2026) and 1H2026 results (9 Sep 2026); Sustainability Report 2025",
    "Zara, Pre-Owned (Resell) US terms and conditions (18 Oct 2024); Zara US help centre: Resell, Repair, Clothes Collection",
    "ThredUp, Resale Report 2025 and 2026 (with GlobalData); FY2025 10-K; Q3 2026 8-K (4 Nov 2026); IPO S-1 (2021); seller terms",
    "Patagonia, \"A new milestone for Patagonia's resale business\" (14 Oct 2026); Work in Progress report (Nov 2025); Trellis, PopSci, GearJunkie",
    "Zalando, release on Pre-owned and the Vestiaire partnership (28 May 2026)",
    "Vestiaire Collective, Trust page and seller-fee FAQ (2025-26); fast-fashion ban release (Nov 2023); CEO interview via FashionUnited (Feb 2026)",
    "The RealReal, Q3 2026 results 8-K (29 Oct 2026) and 2026 earnings calls",
    "H&M Group, Annual and Sustainability Report 2025; Sellpy FY2025 results (ehandel.com, MarketScreener)",
    "Angrist, Caldwell and Hall, \"Uber versus Taxi\", American Economic Journal: Applied (2021); US GAO-20-191 (2020)",
    "Scientific Reports (2025), US clothing disposal survey (n=1,009); BCG x Vestiaire Collective (Oct 2022); YouGov US; WRAP (2022, 2025)",
    "INFORMS M&SOM (2023) on stock-out demand; Retail Week size-availability ranking; Omnisend 2025; Airship and Batch 2025 push benchmarks",
    "USPS Notice 123 (Oct 2026); BLS Employer Costs for Employee Compensation (Dec 2025); Stripe Connect pricing; ZipRecruiter and Inditex Careers (2026)",
    "eBay Developer Program (Buy APIs); Depop Partner API; Bright Data dataset pricing; Mercari Engineering; ISO 12945-2; Sensors (2023)",
    "EU revised Waste Framework Directive (2025); France Law 2026-602; CalRecycle SB 707; Vinted newsroom 2026; Poshmark S-1 (2020)",
  ];
  bullets(s, refs, M, TOP, CW, APX_BOTTOM - TOP, 12, 9);
}

pres.writeFile({ fileName: D + "ZARA_ReLoop_Deck.pptx" }).then(f => console.log("wrote", f, "slides", page));
