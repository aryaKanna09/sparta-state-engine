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
| 10–12 | Seller flow | Notification → purchase history → guided capture |
| **13** | **Live camera grader** | **Actually runs — real camera, real capture coaching** |
| 14–17 | Seller flow | Grade → appeal → drop-off → payout |
| 18 | Six systems | The novelty argument, made falsifiable |

Desktop screens are presented inside a fictional laptop browser window with a
realistic zara.com URL, so exports read as screenshots rather than a web page.

## The live grader (`/grader.html`)

The only screen that is not a mockup. It opens the camera, analyses every frame
in-browser and arms the shutter when the shot is good enough to grade from.

**Real:** camera capture, and per-frame measurement of exposure (mean luminance),
sharpness (variance of the Laplacian) and framing (edge energy inside the guide box
versus outside). The three chips go black when each one passes.

**Simulated:** the condition grade. It is weighted by measured capture quality, so it
reacts to what you photograph, but it is not a trained condition model. Say so if a
judge asks — the honest answer is stronger than the vague one.

**Running it on stage**

- Needs HTTPS or localhost for camera access. The Vercel URL is HTTPS, so a phone works.
- **Press `1`, `2` or `3` before the last shot** to lock the tier to EXCELLENT / GOOD / FAIR
  so the result is predictable in front of judges. The lock is shown under the phone.
- **Space bar** fires the shutter.
- If the camera is blocked or absent it falls back to a bundled photograph and still
  completes the flow — it will not die in front of judges.

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


## Financial model

`model/ZARA_ReLoop_Financial_Model.xlsx` is the three-year US model behind the case (live formulas; `Assumptions!C2` switches Conservative / Base / Upside).
`model/build_model.py` generates it, `model/run_model.py` recalculates every scenario and lever with LibreOffice, `model/finalize.py` assembles the workbook.

    cd model && python3 run_model.py /tmp/reloop && python3 finalize.py /tmp/reloop
