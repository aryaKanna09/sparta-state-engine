# ZARA RELOOP — case competition UI mockup

A clickable, hyper-realistic reconstruction of zara.com and the Zara app, built to
walk judges from the **buyer** side to the **seller** side of the Team Fireworks
concept: when a shopper hits a sold-out size, Zara asks the people who already own
that exact garment whether they want to sell it, and grades and prices it for them.

Static HTML/CSS. No framework, no build dependencies.

```bash
node src/build.js          # regenerate public/ from src/
npx http-server public     # preview locally
```

## The walkthrough

`public/index.html` is the contents page. Every screen has a presenter bar at the
bottom — **arrow keys** move through the flow, so it can be driven hands-free.

| # | Screen | What it proves |
|---|---|---|
| 01 | Homepage | Zara as it is today |
| 02 | Category listing | She finds the blazer |
| 03 | Product page | **The dead end.** Size M struck through |
| 04 | Pre-owned in your size | **The intervention** |
| 05 | Asking the owners | Demand creates supply |
| 06 | Pre-owned product page | Dual imagery + condition report |
| 07 | Pre-owned catalogue | Filterable by condition |
| 08 | Bag and fees | Real Zara Resell economics |
| 09 | Order confirmed | Nothing cannibalised |
| 10–16 | Seller flow | Notification → closet → capture → grade → appeal → drop-off → payout |
| 17 | Six systems | The novelty argument, made falsifiable |

## What is reconstructed vs. what is ours

**Reconstructed from Zara's live site, help centre and the Zara Resell T&Cs:**
the tab list (`WOMAN MAN KIDS ZARA HOME BEAUTY MASSIMO DUTTI PRE-OWNED TRAVEL MODE`),
the vertical right-hand utility stack, 2:3 product imagery, weight-300 type, the
sold-out strikethrough, the separate `Coming soon` state, the `Blue 2757/397/403`
reference line, the Pre-Owned pillars (Repair / Resell / Donation), and the buyer-paid
**$1.00 + 5%** transaction fee with the seller keeping 100% of their asking price.

**Ours (new):** the demand trigger on the PDP, the purchase-history match, guided
capture, the three-tier grade, the price band, and the condition filter.

> **Label on the slide:** Zara does not publish a condition ladder, so
> EXCELLENT / GOOD / FAIR is a **team assumption**. Their T&Cs do bound it — the top
> tier is gated to "brand new and unworn" and listing anything "in very poor
> condition" is forbidden — so a three-tier scale sits correctly inside their terms.

## Changing the feature name

One constant. `src/data.js` → `COPY.featureName`, then `node src/build.js`.

## Images

Product photography is Zara's own, pulled from their CDN, normalised to their 2:3
ratio and used to depict their UI. The seller "phone photos" are the same blazer
re-processed (crop, tilt, colour cast, grain) to read as three different owners'
snapshots — which is accurate, since all three listings are the same SKU.

To swap in different garments, drop a JPG into `public/assets/img/` using the name
referenced in `src/data.js` and rebuild. Any missing file degrades to a styled
placeholder rather than a broken image, so the deck never shows a broken tile.
