"""ZARA RELOOP - US 3-year financial model. Builds an .xlsx with live formulas.
Usage: python3 build_model.py out.xlsx [scenario 1|2|3] [key=value overrides applied to the Base column]"""
import sys
from openpyxl import Workbook
from openpyxl.styles import Font, PatternFill, Alignment
from openpyxl.utils import get_column_letter

# key, label, unit, conservative, base, upside, source / confidence
A = [
 ("H1", "VOLUME FUNNEL (United States)"),
 ("cust", "Identified US customers with a digital closet (online orders + app / e-receipt purchases)", "millions", 6.0, 8.0, 10.0, "ESTIMATE. US revenue is not disclosed (2nd-largest market, below Spain's ~EUR 6.4bn). Assumes ~$4bn US sales / ~$300 annual spend x ~60% identifiable."),
 ("cust_g", "Annual growth of identified base", "%", 0.05, 0.08, 0.10, "ESTIMATE. Inditex 1H2026 sales +7.6%; US store count 103 -> 110."),
 ("cov1", "Rollout coverage, Year 1 (pilot metros months 1-6, then national)", "%", 0.25, 0.35, 0.45, "DESIGN. Matches the 6-month rollout plan."),
 ("cov2", "Rollout coverage, Year 2", "%", 1.0, 1.0, 1.0, "DESIGN"),
 ("cov3", "Rollout coverage, Year 3", "%", 1.0, 1.0, 1.0, "DESIGN"),
 ("reach", "Push opt-in / reachable share", "%", 0.49, 0.55, 0.61, "Airship 2025 iOS median 48.9%; Batch 2025 iOS 56%, overall 61%."),
 ("pr1", "Price-led prompts per reached customer, Year 1 (frequency cap)", "#", 3, 4, 4, "DESIGN. Hard cap so the prompt stays rare and valuable."),
 ("pr2", "Prompts per reached customer, Years 2-3", "#", 4, 6, 8, "DESIGN. Max one every ~2 months in base."),
 ("conv", "Prompt -> listing started", "%", 0.015, 0.03, 0.05, "ANALOGY (no direct benchmark exists). E-commerce push CTR 3.1-3.8%; abandoned-cart flow 3.3% order rate; back-in-stock 5.3-6.7% conversion; triggered push opens 14.4% vs 4.2% generic."),
 ("conv_up", "Conversion learning uplift by Year 3", "%", 0.0, 0.10, 0.20, "ESTIMATE"),
 ("complete", "Listing completion (4 photos + accept the price)", "%", 0.60, 0.70, 0.80, "ESTIMATE. Poshmark Smart List AI cut listing time 48%; 33% of resellers say photographing/listing is the most frustrating task."),
 ("items", "Items listed per completed lister-session (the closet view shows every priced item)", "#", 1.2, 1.5, 2.0, "ESTIMATE. WRAP: 26% of a wardrobe (31 of 118 items) unworn in the past year."),
 ("organic", "Organic (un-prompted) listings as % of prompted", "%", 0.10, 0.15, 0.25, "ESTIMATE"),
 ("sell", "Sell-through inside the 30-day offer window", "%", 0.45, 0.60, 0.70, "Zara median 24 days to sell across marketplaces; 11 days on Vinted France. Priced-to-market by the engine."),
 ("price", "Average pre-owned selling price", "$", 30.0, 36.0, 40.0, "Zara median resale ~$20 all categories; ReLoop price floor $20 skews to blazers/coats/dresses/knits. Mock listings average ~$44."),
 ("H2", "FEES CHARGED"),
 ("fee_fix", "Buyer fee - fixed part", "$", 1.0, 1.0, 1.0, "CONFIRMED. Zara Pre-Owned US terms: $1 + 5%."),
 ("fee1", "Buyer fee - % of price, Year 1", "%", 0.05, 0.05, 0.05, "CONFIRMED current fee."),
 ("fee2", "Buyer fee - % of price, Year 2 (verified-service fee test)", "%", 0.05, 0.08, 0.10, "DESIGN. Benchmarks: COS Resell 10%, Vestiaire 12%, eBay 13.6%, Poshmark 20%."),
 ("fee3", "Buyer fee - % of price, Year 3", "%", 0.05, 0.10, 0.10, "DESIGN"),
 ("ship_fee", "Shipping charged to the buyer", "$", 4.95, 4.95, 4.95, "DESIGN: charged at cost, as Zara Pre-Owned does today (buyer pays the label). Zara standard delivery is $4.95; Poshmark charges buyers $8.27."),
 ("mail_fee", "Mail-in deduction charged to the seller (store drop-off is free)", "$", 4.95, 4.95, 4.95, "DESIGN mirroring Zara's returns policy: mail return $4.95, in-store free (CONFIRMED)."),
 ("H3", "VARIABLE COST TO SERVE"),
 ("parcel", "Parcel cost per leg (1 lb poly mailer)", "$", 8.50, 6.50, 5.50, "USPS Ground Advantage commercial 1 lb $7.33-9.29 list (Oct 2026); Pitney Bowes US average $9.09 per parcel; base assumes a large-shipper negotiated rate (ESTIMATE)."),
 ("drop", "Share of sellers who drop off in store", "%", 0.40, 0.50, 0.60, "PayPal/TRC: 54% prefer in-person drop-off; limited by 102 US stores."),
 ("lab_min", "Staff minutes per item (verify 3 + pack/label 3)", "min", 8.0, 6.0, 4.5, "Returns benchmarks ~4 min per RMA (ShipMonk/Racklify); inspection 1-3 min."),
 ("lab_rate", "Loaded labour cost per minute", "$", 0.46, 0.40, 0.37, "Zara US postings $16-21/hr (avg $18.21-18.50) x 1.30 BLS retail benefits load = $24.10/hr."),
 ("pack", "Packaging per parcel", "$", 0.40, 0.15, 0.10, "Poly mailer $0.04-0.11 + label."),
 ("pay_pct", "Buyer payment processing %", "%", 0.029, 0.022, 0.018, "Stripe list 2.9% + $0.30; base assumes enterprise pricing (ESTIMATE)."),
 ("pay_fix", "Buyer payment processing fixed", "$", 0.30, 0.20, 0.10, "as above"),
 ("acct_fee", "Stripe Connect fee per active payout account-month", "$", 2.0, 2.0, 2.0, "CONFIRMED Stripe Connect list: $2 per monthly active account + 0.25% + $0.25 per payout."),
 ("u_per_acct", "Units paid out per active seller-month", "#", 1.0, 1.3, 1.6, "ESTIMATE"),
 ("ai", "AI compute per item (6 images, vision grading + pricing)", "$", 0.05, 0.03, 0.01, "Derived from 2026 list prices: ~$0.001 (small model) to ~$0.025 (mid-tier) per item."),
 ("exc_rate", "Exception rate (grade dispute, no-show, re-ship)", "%", 0.06, 0.04, 0.03, "ESTIMATE. Vestiaire rejects ~10% at authentication; Sift marketplace fraud attack rate 1.6%."),
 ("exc_cost", "Cost per exception", "$", 14.0, 12.0, 10.0, "ESTIMATE: extra leg + re-grade + goodwill."),
 ("cs", "Customer service per unit", "$", 0.50, 0.30, 0.20, "ESTIMATE"),
 ("H4", "LOOP EFFECTS (where the value is)"),
 ("cred_share", "Share of sellers taking payout as Zara credit", "%", 0.45, 0.60, 0.70, "Credit is instant and carries a bonus. Benchmarks: brand trade-in programmes are 100% credit; The RealReal +5%, Fashionphile +10% for credit."),
 ("cred_bonus", "Bonus for choosing Zara credit over cash", "%", 0.10, 0.10, 0.10, "DESIGN, benchmark above."),
 ("breakage", "Credit never redeemed", "%", 0.03, 0.05, 0.08, "Weak sourcing: ~6% of gift cards never used; Mercator ~3%."),
 ("incr", "Share of credit redemptions that are incremental orders", "%", 0.25, 0.40, 0.55, "ASSUMPTION - the single most important number to measure in the pilot (hold-out test)."),
 ("over", "Overspend above the credit on incremental orders", "%", 0.30, 0.50, 0.70, "Gift-card proxy: 61% of redeemers overspend, by $31.75 (Capital One) to $59 (First Data 2018) on average."),
 ("cm", "Flow-through margin on incremental new-merchandise sales", "%", 0.45, 0.50, 0.55, "Inditex gross margin 58.3% FY2025 (CONFIRMED) less variable selling cost."),
 ("cogs", "Cost of goods as % of sales", "%", 0.417, 0.417, 0.417, "1 - 58.3% gross margin."),
 ("shA", "Mix: sold-out-size / sold-out-style trigger (bucket A)", "%", 0.30, 0.35, 0.40, "ESTIMATE"),
 ("shB", "Mix: in-stock current-season item (bucket B)", "%", 0.15, 0.10, 0.10, "ESTIMATE; supply is naturally thin. Bucket C (past season) is the remainder."),
 ("cA", "Cannibalisation, bucket A (would have bought another new Zara item instead)", "%", 0.35, 0.25, 0.15, "INFORMS M&SOM 2023: 51.7% of unmet size demand spills to adjacent sizes, ~48% is lost to the style. Pre-owned takers skew to the lost half."),
 ("cB", "Cannibalisation, bucket B", "%", 0.70, 0.60, 0.50, "ESTIMATE; WRAP: resale displaces 64.6% of new purchases market-wide."),
 ("cC", "Cannibalisation, bucket C", "%", 0.15, 0.10, 0.05, "ESTIMATE; 46% of shoppers already browse resale before buying new, so that leak exists today off-platform."),
 ("new_price", "Average price of the new item displaced", "$", 55.0, 55.0, 55.0, "ESTIMATE; a Zara dress ~$48 (Statista 2022)."),
 ("save_rate", "Bucket A: share of orders where the rest of the basket is saved", "%", 0.08, 0.15, 0.20, "65% of online shoppers hitting a stock-out go elsewhere or do not buy (GT Nexus/YouGov); 31% buy at another store (Corsten & Gruen)."),
 ("save_val", "Value of the saved new-item basket", "$", 60.0, 75.0, 90.0, "Zara AOV $110 net (ECDB) less the hero item."),
 ("att_rate", "All buckets: pre-owned orders with an incremental new add-on (one native bag)", "%", 0.05, 0.10, 0.15, "ESTIMATE. Patagonia: resale on the main site converted ~1.5x higher."),
 ("att_val", "Value of the add-on", "$", 40.0, 45.0, 50.0, "ESTIMATE"),
 ("ntb", "Pre-owned buyers who are new to Zara", "%", 0.10, 0.20, 0.30, "Patagonia 33%; Trove 40-60% and Archive ~50% (vendor claims); sceptic 15-30%."),
 ("ntb_conv", "New-to-brand buyers who buy new within 12 months", "%", 0.10, 0.25, 0.30, "Trove: 10% within 6 months, up to 30% within a year."),
 ("ntb_spend", "Their first-year new-merchandise spend", "$", 120.0, 150.0, 180.0, "ESTIMATE"),
 ("u_per_buyer", "Pre-owned units per buyer per year", "#", 1.5, 1.5, 1.5, "ESTIMATE; ThredUp 3.7 orders per active buyer."),
 ("bonus_face", "Average dynamic demand bonus paid (bucket A listings that sell)", "$", 5.0, 4.0, 4.0, "DESIGN. Range $0-10, weighted average ~$5 on the CreditEngine sheet. Benchmarks: Lululemon $5/10/25, Levi's $5-30, Madewell $20."),
 ("H5", "FIXED COST AND CAPITAL"),
 ("fix1", "Incremental fixed operating cost, Year 1", "$m", 1.2, 1.0, 1.0, "ESTIMATE: pricing/ML (3 FTE), ops programme (3), trust & safety/CS (4), data licences, cloud. Incremental to today's Pre-Owned team."),
 ("fix2", "Incremental fixed operating cost, Year 2", "$m", 2.0, 1.6, 1.6, "ESTIMATE"),
 ("fix3", "Incremental fixed operating cost, Year 3", "$m", 2.2, 1.7, 1.8, "ESTIMATE"),
 ("capex_mult", "Capex multiplier on the Capex sheet total", "x", 1.25, 1.0, 0.9, "Scenario contingency."),
 ("disc", "Discount rate", "%", 0.08, 0.08, 0.08, "ESTIMATE"),
 ("co2", "Net CO2e avoided per resold item", "kg", 1.8, 4.0, 6.9, "Vinted/Vaayu measured 1.8 kg; WRAP displacement 64.6% x ~11.5 kg garment - 0.5 kg parcel = 6.9 kg."),
 ("stores", "Zara stores in the US", "#", 102, 102, 102, "CONFIRMED end FY2025."),
]

# Scenario ranges are joint outcomes, anchored on published benchmarks for brand-run resale:
# Conservative ~ the typical brand programme (<0.1% of sales), Base ~ 0.2% of US sales,
# Upside ~ what H&M group (0.8%) and Patagonia (~0.9%) reached after years.
REV = {
 "cust": (5.0, 7.0, 9.0), "cust_g": (0.04, 0.06, 0.08), "reach": (0.50, 0.55, 0.58), "pr1": (3, 4, 4), "pr2": (4, 6, 6),
 "conv": (0.008, 0.015, 0.03), "conv_up": (0.0, 0.10, 0.10), "complete": (0.60, 0.65, 0.70), "items": (1.2, 1.4, 1.5),
 "organic": (0.10, 0.15, 0.15), "sell": (0.45, 0.55, 0.60), "price": (26.0, 30.0, 36.0), "fee2": (0.05, 0.10, 0.10), "fee3": (0.05, 0.10, 0.10),
 "ship_fee": (6.50, 6.50, 6.50), "parcel": (7.50, 6.50, 6.00), "drop": (0.45, 0.50, 0.55), "lab_min": (10.0, 7.0, 6.0),
 "lab_rate": (0.43, 0.40, 0.40), "pack": (0.25, 0.15, 0.12), "pay_pct": (0.025, 0.022, 0.020), "pay_fix": (0.25, 0.20, 0.15),
 "u_per_acct": (1.2, 1.3, 1.5), "exc_rate": (0.06, 0.04, 0.04), "exc_cost": (13.0, 12.0, 12.0), "cs": (0.40, 0.30, 0.30),
 "cred_share": (0.55, 0.70, 0.70), "breakage": (0.04, 0.05, 0.05), "incr": (0.30, 0.40, 0.40), "over": (0.40, 0.50, 0.50),
 "cm": (0.47, 0.50, 0.50), "shA": (0.30, 0.35, 0.35), "shB": (0.13, 0.10, 0.10), "cA": (0.30, 0.25, 0.25), "cB": (0.65, 0.60, 0.60),
 "cC": (0.13, 0.10, 0.10), "save_rate": (0.10, 0.15, 0.15), "save_val": (65.0, 75.0, 75.0), "att_rate": (0.07, 0.10, 0.12),
 "att_val": (42.0, 45.0, 45.0), "ntb": (0.13, 0.20, 0.20), "ntb_conv": (0.15, 0.25, 0.25), "ntb_spend": (130.0, 150.0, 150.0),
 "bonus_face": (5.0, 5.0, 5.0), "fix1": (0.9, 0.8, 1.0), "fix2": (1.3, 1.1, 1.6), "fix3": (1.4, 1.2, 1.7), "capex_mult": (1.15, 1.0, 1.0), "ai": (0.04, 0.03, 0.03),
}
SRC = {
 "cust": "ESTIMATE, plausible range 4-13M. Inditex does not disclose US sales or customers. Order-based: ECDB US online ~$1.45bn / $90-120 per order / 2-3 orders a year = 4-8M online buyers; app-based: 11-13M.",
 "conv": "ANALOGY, no direct benchmark exists. Passive benchmark: 2.3% of Shein Exchange sign-ups listed anything in year one. Push: e-commerce CTR 3.1-3.8%, abandoned-cart order rate 3.3%, back-in-stock 5.3-6.7%. Base 1.5% per prompt x 6 prompts gives roughly 5% of customers listing in a year.",
 "price": "Zara's typical resale price is low: ~EUR 12.80 on Vinted (1.1M sales), ~$20 median across marketplaces. ReLoop has a $20 floor and skews to jackets, coats and dresses, so $30 base. $36 only in the upside.",
 "sell": "Zara median 24 days to sell across marketplaces, 11 days on Vinted France. Zalando Pre-owned: 62% of the range sells within 7 days (company figure, 2026).",
 "att_rate": "ASSUMPTION. Zalando Pre-owned (2026): 40% of customers buying pre-owned add a NEW item to the same order. We count a quarter of that as incremental.",
 "ntb": "ASSUMPTION. No brand publishes this; vendor claims range from 40% to 80% (Trove, Archive) and contradict each other. 20% is half the lowest vendor figure.",
 "ntb_conv": "ASSUMPTION. Vendor claims: Trove 10% within 6 months in its report (its CEO has said 50%); Archive 47-60%.",
 "over": "ASSUMPTION. Gift-card proxy: 61% of redeemers overspend, by $31.75 (Capital One) to $59 (First Data 2018). Vendor claims for resale credit are far higher (Treet: recipients spend 2.5-3x the credit; M.M.LaFleur ~3x).",
 "cred_share": "ASSUMPTION. Brand trade-in programmes are 100% credit; M.M.LaFleur reports ~70% of sellers choose credit (Modern Retail); Lululemon Canada pays 70% cash or 90% credit.",
 "lab_min": "ESTIMATE: verify 3-4 min + pack/label 3. Reference points: returns ~4 min per RMA; Patagonia's full managed operation (inspect, clean, repair, photograph, ship) runs ~230,000 items a year with 40+ associates, roughly 20 min an item.",
 "fix1": "ESTIMATE, incremental to today's Pre-Owned team: pricing/ML 2 FTE, ops programme 2, trust & safety/CS 2, data licences, cloud. Scales with volume across scenarios.",
}
A = [row if len(row) == 2 else (row[0], row[1], row[2]) + tuple(REV.get(row[0], row[3:6])) + (SRC.get(row[0], row[6]),) for row in A]

def build(path, scenario=2, over=None):
    over = over or {}
    wb = Workbook()
    bold = Font(bold=True); hdr = PatternFill("solid", fgColor="111111"); hf = Font(bold=True, color="FFFFFF")
    sec = PatternFill("solid", fgColor="E8E8E8"); inp = Font(color="0000CC")
    ws = wb.active; ws.title = "Assumptions"
    ws["A1"] = "ZARA RELOOP - US financial model. Blue = input. Change the scenario cell to flip every sheet."; ws["A1"].font = Font(bold=True, size=13)
    ws["B2"] = "Scenario (1 = Conservative, 2 = Base, 3 = Upside)"; ws["C2"] = scenario; ws["C2"].font = Font(bold=True, color="0000CC", size=13)
    for i, h in enumerate(["key", "Assumption", "Unit", "Conservative", "Base", "Upside", "SELECTED", "Source / confidence"], 1):
        c = ws.cell(row=4, column=i, value=h); c.font = hf; c.fill = hdr
    R = {}; r = 5
    for row in A:
        if len(row) == 2:
            ws.cell(row=r, column=2, value=row[1]).font = bold
            for c in range(1, 9): ws.cell(row=r, column=c).fill = sec
        else:
            k, label, unit, lo, ba, hi, src = row
            if k in over: ba = over[k]
            ws.cell(row=r, column=1, value=k); ws.cell(row=r, column=2, value=label); ws.cell(row=r, column=3, value=unit)
            for j, v in enumerate((lo, ba, hi)):
                c = ws.cell(row=r, column=4 + j, value=v); c.font = inp
                c.number_format = "0.0%" if unit == "%" else ("0.00" if unit in ("$", "min", "x", "kg", "$m", "millions") else "0.0")
            s = ws.cell(row=r, column=7, value="=CHOOSE($C$2,D%d,E%d,F%d)" % (r, r, r)); s.font = bold
            s.number_format = ws.cell(row=r, column=5).number_format
            ws.cell(row=r, column=8, value=src)
            R[k] = "Assumptions!$G$%d" % r
        r += 1
    for col, w in zip("ABCDEFGH", (11, 78, 9, 13, 10, 10, 11, 120)): ws.column_dimensions[col].width = w
    ws.freeze_panes = "C5"

    # ---------- Capex
    cx = wb.create_sheet("Capex")
    cx["A1"] = "One-time investment (US launch; the build is reusable in the other 16 Pre-Owned markets)"; cx["A1"].font = Font(bold=True, size=13)
    for i, h in enumerate(["Item", "Basis", "$"], 1):
        c = cx.cell(row=3, column=i, value=h); c.font = hf; c.fill = hdr
    items = [
     ("Native app integration (replace the webview: PDP button, one bag/checkout, seller flow, camera capture) iOS + Android", "12 FTE x 9 months x $14k loaded", "=12*9*14000"),
     ("Pricing engine, comps pipeline, demand-bonus engine", "5 FTE x 9 months x $16k", "=5*9*16000"),
     ("Vision grading: rubric, 20k staff-labelled items, model integration, QA", "lump sum", 500000),
     ("Store kit: inspection/photo station, bins, mailers, signage", "$2,500 x 102 stores", "=2500*102"),
     ("Store training", "102 stores x 10 associates x 2 h x $24.10 + content", "=102*10*2*24.1+100000"),
     ("Mail-in verification hubs (2, inside existing facilities)", "lump sum", 200000),
     ("Payments, credit ledger, sales-tax (marketplace facilitator), legal terms", "lump sum", 300000),
     ("Launch marketing (owned channels first)", "lump sum", 500000),
    ]
    rr = 4
    for it in items:
        cx.cell(row=rr, column=1, value=it[0]); cx.cell(row=rr, column=2, value=it[1])
        c = cx.cell(row=rr, column=3, value=it[2]); c.number_format = "#,##0"; rr += 1
    cx.cell(row=rr, column=1, value="Contingency 15%"); c = cx.cell(row=rr, column=3, value="=0.15*SUM(C4:C%d)" % (rr - 1)); c.number_format = "#,##0"; rr += 1
    cx.cell(row=rr, column=1, value="TOTAL CAPEX").font = bold; c = cx.cell(row=rr, column=3, value="=SUM(C4:C%d)" % (rr - 1)); c.number_format = "#,##0"; c.font = bold
    CAPEX = "Capex!$C$%d" % rr
    cx.column_dimensions["A"].width = 100; cx.column_dimensions["B"].width = 55; cx.column_dimensions["C"].width = 14

    # ---------- Unit economics
    ue = wb.create_sheet("UnitEconomics")
    ue["A1"] = "Per pre-owned unit sold ($). Formulas read the SELECTED column."; ue["A1"].font = Font(bold=True, size=13)
    for i, h in enumerate(["Line", "Year 1", "Year 2", "Year 3", "Note"], 1):
        c = ue.cell(row=3, column=i, value=h); c.font = hf; c.fill = hdr
    U = {}; state = {"r": 4}
    def line(key, label, f1, f2=None, f3=None, note="", b=False, fmt="0.00"):
        r_ = state["r"]
        ue.cell(row=r_, column=1, value=label).font = bold if b else Font()
        for j, f in enumerate((f1, f2 if f2 is not None else f1, f3 if f3 is not None else f1)):
            col = "BCD"[j]
            c = ue.cell(row=r_, column=2 + j, value="=" + f.replace("@", col)); c.number_format = fmt
            if b: c.font = bold
        ue.cell(row=r_, column=5, value=note); U[key] = r_; state["r"] += 1
    def u(k): return "@%d" % U[k]
    def head(t):
        ue.cell(row=state["r"], column=1, value=t).font = bold
        for c in range(1, 6): ue.cell(row=state["r"], column=c).fill = sec
        state["r"] += 1
    head("TRANSACTION")
    line("price", "Selling price", R["price"])
    line("fee", "Buyer fee", R["fee_fix"] + "+" + R["fee1"] + "*" + u("price"), R["fee_fix"] + "+" + R["fee2"] + "*" + u("price"), R["fee_fix"] + "+" + R["fee3"] + "*" + u("price"), "fixed + % of price")
    line("ship", "Shipping charged to buyer", R["ship_fee"])
    line("mailrev", "Mail-in deduction from seller", "(1-" + R["drop"] + ")*" + R["mail_fee"])
    line("rev", "Platform revenue", u("fee") + "+" + u("ship") + "+" + u("mailrev"), b=True)
    line("out", "Outbound parcel", "-" + R["parcel"])
    line("inb", "Inbound parcel (mail-in share only)", "-(1-" + R["drop"] + ")*" + R["parcel"])
    line("lab", "Store / hub labour", "-" + R["lab_min"] + "*" + R["lab_rate"])
    line("pk", "Packaging", "-" + R["pack"])
    line("pay", "Buyer payment processing", "-(" + R["pay_pct"] + "*(" + u("price") + "+" + u("fee") + "+" + u("ship") + ")+" + R["pay_fix"] + ")")
    line("po", "Cash payout cost (credit payouts cost nothing)", "-(1-" + R["cred_share"] + ")*(" + R["acct_fee"] + "/" + R["u_per_acct"] + "+0.25+0.0025*" + u("price") + ")")
    line("aic", "AI compute", "-" + R["ai"])
    line("exc", "Exceptions", "-" + R["exc_rate"] + "*" + R["exc_cost"])
    line("csc", "Customer service", "-" + R["cs"])
    line("cost", "Cost to serve", "SUM(@%d:@%d)" % (state["r"] - 9, state["r"] - 1), b=True)
    line("txn", "TRANSACTION CONTRIBUTION", u("rev") + "+" + u("cost"), b=True, note="Negative: at fast-fashion prices a fee cannot pay for managed resale. Everything below is why it is still worth doing.")
    head("LOOP EFFECTS")
    line("cl_sales", "Incremental new sales from credit redemption", u("price") + "*" + R["cred_share"] + "*(1-" + R["breakage"] + ")*" + R["incr"] + "*(1+" + R["over"] + ")")
    line("cl_m", "  margin on those sales", u("cl_sales") + "*" + R["cm"])
    line("cfac", "Cost of $1 of bonus credit", "(1-" + R["breakage"] + ")*(" + R["incr"] + "*" + R["cogs"] + "+(1-" + R["incr"] + "))", note="42c if the redemption is incremental, 100c if it discounts a purchase that would have happened anyway.", fmt="0.000")
    line("pb", "Credit-payout bonus cost", "-" + R["cred_share"] + "*" + u("price") + "*" + R["cred_bonus"] + "*" + u("cfac"))
    line("db", "Dynamic demand bonus cost", "-" + R["shA"] + "*" + R["bonus_face"] + "*" + u("cfac"))
    line("bk_sales", "Incremental new sales: saved baskets + add-ons", R["shA"] + "*" + R["save_rate"] + "*" + R["save_val"] + "+" + R["att_rate"] + "*" + R["att_val"])
    line("bk_m", "  margin on those sales", u("bk_sales") + "*" + R["cm"])
    line("nb_sales", "Incremental new sales: new-to-brand buyers", R["ntb"] + "*" + R["ntb_conv"] + "*" + R["ntb_spend"] + "/" + R["u_per_buyer"])
    line("nb_m", "  margin on those sales", u("nb_sales") + "*" + R["cm"])
    line("can_r", "Blended cannibalisation rate", R["shA"] + "*" + R["cA"] + "+" + R["shB"] + "*" + R["cB"] + "+(1-" + R["shA"] + "-" + R["shB"] + ")*" + R["cC"], fmt="0.0%")
    line("can_sales", "New sales displaced", "-" + u("can_r") + "*" + R["new_price"])
    line("can_m", "  margin lost", u("can_sales") + "*" + R["cm"])
    head("RESULT")
    line("net", "NET CONTRIBUTION PER UNIT", u("txn") + "+" + u("cl_m") + "+" + u("pb") + "+" + u("db") + "+" + u("bk_m") + "+" + u("nb_m") + "+" + u("can_m"), b=True)
    line("incr_sales", "Net incremental new-merchandise sales per unit", u("cl_sales") + "+" + u("bk_sales") + "+" + u("nb_sales") + "+" + u("can_sales"), b=True)
    line("zrev", "Net incremental Zara revenue per unit (platform revenue + net new sales)", u("rev") + "+" + u("incr_sales"), b=True)
    line("credit_face", "Zara credit issued per unit (payout credit + bonuses)", R["cred_share"] + "*" + u("price") + "*(1+" + R["cred_bonus"] + ")+" + R["shA"] + "*" + R["bonus_face"])
    line("bonus_face_u", "Bonus credit face value per unit", R["cred_share"] + "*" + u("price") + "*" + R["cred_bonus"] + "+" + R["shA"] + "*" + R["bonus_face"])
    ue.column_dimensions["A"].width = 70
    for col in "BCD": ue.column_dimensions[col].width = 12
    ue.column_dimensions["E"].width = 110
    def UE(k, col): return "UnitEconomics!%s%d" % (col, U[k])

    # ---------- Model
    m = wb.create_sheet("Model")
    m["A1"] = "Three-year model, United States"; m["A1"].font = Font(bold=True, size=13)
    for i, h in enumerate(["Line", "Year 0", "Year 1", "Year 2", "Year 3", "Total"], 1):
        c = m.cell(row=3, column=i, value=h); c.font = hf; c.fill = hdr
    M = {}; ms = {"r": 4}
    def ml(key, label, y1, y2, y3, y0=None, fmt="#,##0", total=True, b=False):
        r_ = ms["r"]
        m.cell(row=r_, column=1, value=label).font = bold if b else Font()
        vals = [y0, y1, y2, y3]
        for j, f in enumerate(vals):
            if f is None: continue
            c = m.cell(row=r_, column=2 + j, value="=" + f); c.number_format = fmt
            if b: c.font = bold
        if total:
            c = m.cell(row=r_, column=6, value="=SUM(B%d:E%d)" % (r_, r_)); c.number_format = fmt
            if b: c.font = bold
        M[key] = r_; ms["r"] += 1
    def mr(k, col): return "%s%d" % (col, M[k])
    def per(k, f):  # same formula pattern across Y1..Y3; '@' = column letter, '#' = UnitEconomics column
        return [f.replace("@", c).replace("#", uc) for c, uc in (("C", "B"), ("D", "C"), ("E", "D"))]
    def mh(t):
        m.cell(row=ms["r"], column=1, value=t).font = bold
        for c in range(1, 7): m.cell(row=ms["r"], column=c).fill = sec
        ms["r"] += 1
    mh("FUNNEL")
    ml("cust", "Identified customers with a digital closet", R["cust"] + "*1000000", "C%d*(1+%s)" % (ms["r"], R["cust_g"]), "D%d*(1+%s)" % (ms["r"], R["cust_g"]), total=False)
    ml("cov", "Rollout coverage", R["cov1"], R["cov2"], R["cov3"], fmt="0%", total=False)
    ml("reach", "Reachable share", R["reach"], R["reach"], R["reach"], fmt="0%", total=False)
    ml("pr", "Prompts per reached customer", R["pr1"], R["pr2"], R["pr2"], fmt="0.0", total=False)
    ml("prompts", "Price-led prompts delivered", *per("p", "@%d*@%d*@%d*@%d" % (M["cust"], M["cov"], M["reach"], M["pr"])))
    ml("conv", "Prompt -> listing started", R["conv"], R["conv"], R["conv"] + "*(1+" + R["conv_up"] + ")", fmt="0.0%", total=False)
    ml("starts", "Listing sessions started", *per("s", "@%d*@%d" % (M["prompts"], M["conv"])))
    ml("lst_p", "Prompted listings completed", *per("l", "@%d*%s*%s" % (M["starts"], R["complete"], R["items"])))
    ml("lst", "Total listings (incl. organic)", *per("l", "@%d*(1+%s)" % (M["lst_p"], R["organic"])))
    ml("units", "UNITS SOLD (garments given a second life)", *per("u", "@%d*%s" % (M["lst"], R["sell"])), b=True)
    ml("gmv", "GMV = paid to sellers ($)", *per("g", "@%d*%s" % (M["units"], UE("price", "#"))), b=True)
    mh("ZARA REVENUE ($)")
    ml("prev", "Platform revenue (fees + shipping)", *per("x", "@%d*%s" % (M["units"], UE("rev", "#"))))
    ml("isales", "Incremental new-merchandise sales (credit loop + baskets + new-to-brand)", *per("x", "@%d*(%s+%s+%s)" % (M["units"], UE("cl_sales", "#"), UE("bk_sales", "#"), UE("nb_sales", "#"))))
    ml("csales", "New-merchandise sales displaced", *per("x", "@%d*%s" % (M["units"], UE("can_sales", "#"))))
    ml("zrev", "NET INCREMENTAL ZARA REVENUE", *per("x", "@%d+@%d+@%d" % (M["prev"], M["isales"], M["csales"])), b=True)
    mh("CONTRIBUTION ($)")
    ml("txn", "Transaction contribution", *per("x", "@%d*%s" % (M["units"], UE("txn", "#"))))
    ml("loopm", "Margin on incremental new sales", *per("x", "@%d*(%s+%s+%s)" % (M["units"], UE("cl_m", "#"), UE("bk_m", "#"), UE("nb_m", "#"))))
    ml("inc", "Incentive cost (credit bonus + demand bonus)", *per("x", "@%d*(%s+%s)" % (M["units"], UE("pb", "#"), UE("db", "#"))))
    ml("canm", "Margin lost to cannibalisation", *per("x", "@%d*%s" % (M["units"], UE("can_m", "#"))))
    ml("vc", "VARIABLE CONTRIBUTION", *per("x", "@%d+@%d+@%d+@%d" % (M["txn"], M["loopm"], M["inc"], M["canm"])), b=True)
    ml("fix", "Incremental fixed operating cost", "-" + R["fix1"] + "*1000000", "-" + R["fix2"] + "*1000000", "-" + R["fix3"] + "*1000000")
    ml("op", "OPERATING CONTRIBUTION", *per("x", "@%d+@%d" % (M["vc"], M["fix"])), b=True)
    ml("capex", "Capex", "0", "0", "0", y0="-" + CAPEX + "*" + R["capex_mult"])
    ml("cf", "NET CASH FLOW", "C%d+C%d" % (M["op"], M["capex"]), "D%d+D%d" % (M["op"], M["capex"]), "E%d+E%d" % (M["op"], M["capex"]), y0="B%d" % M["capex"], b=True)
    r_ = ms["r"]; m.cell(row=r_, column=1, value="Cumulative cash")
    m.cell(row=r_, column=2, value="=B%d" % M["cf"]).number_format = "#,##0"
    for col, prev in (("C", "B"), ("D", "C"), ("E", "D")):
        m.cell(row=r_, column="ABCDE".index(col) + 1, value="=%s%d+%s%d" % (prev, r_, col, M["cf"])).number_format = "#,##0"
    M["cum"] = r_; ms["r"] += 1
    r_ = ms["r"]; m.cell(row=r_, column=1, value="NPV of the three years at the discount rate").font = bold
    m.cell(row=r_, column=2, value="=B%d+NPV(%s,C%d:E%d)" % (M["cf"], R["disc"], M["cf"], M["cf"])).number_format = "#,##0"; M["npv"] = r_; ms["r"] += 1
    r_ = ms["r"]; m.cell(row=r_, column=1, value="Years of Year-3 run-rate needed to repay capex (blank = never at this run-rate)")
    m.cell(row=r_, column=2, value='=IF(E%d>0,-B%d/E%d,"")' % (M["op"], M["capex"], M["op"])).number_format = "0.0"; M["payback"] = r_; ms["r"] += 1
    mh("OPERATING AND IMPACT METRICS")
    ml("credit", "Zara credit issued ($ face)", *per("x", "@%d*%s" % (M["units"], UE("credit_face", "#"))))
    ml("bonus", "of which bonus credit ($ face)", *per("x", "@%d*%s" % (M["units"], UE("bonus_face_u", "#"))))
    ml("bonus_pct", "Bonus credit as % of GMV", *per("x", "@%d/@%d" % (M["bonus"], M["gmv"])), fmt="0.0%", total=False)
    ml("perunit", "Net contribution per unit after fixed cost ($)", *per("x", "@%d/@%d" % (M["op"], M["units"])), fmt="0.00", total=False)
    ml("hours", "Store / hub labour hours", *per("x", "@%d*%s/60" % (M["units"], R["lab_min"])))
    ml("perstore", "Items per store per day", *per("x", "@%d/%s/364" % (M["units"], R["stores"])), fmt="0.0", total=False)
    ml("co2", "Net CO2e avoided (tonnes)", *per("x", "@%d*%s/1000" % (M["units"], R["co2"])))
    ml("be_units", "Units needed to cover fixed cost at that year's unit contribution", *per("x", 'IF(%s>0,-@%d/%s,"n/a")' % (UE("net", "#"), M["fix"], UE("net", "#"))), total=False)
    m.column_dimensions["A"].width = 78
    for col in "BCDEF": m.column_dimensions[col].width = 16
    m.freeze_panes = "B4"

    # ---------- Cost to serve vs a central-DC managed model
    cs = wb.create_sheet("CostToServe")
    cs["A1"] = "Cost to put one verified pre-owned garment in a buyer's hands ($): ReLoop store model vs a central-warehouse managed-resale model"; cs["A1"].font = Font(bold=True, size=13)
    for i, h in enumerate(["Line", "ReLoop (stores, sell first)", "Central DC (intake first)", "Note"], 1):
        c = cs.cell(row=3, column=i, value=h); c.font = hf; c.fill = hdr
    rows = [
     ("Inbound parcel", "=(1-%s)*%s" % (R["drop"], R["parcel"]), "=%s" % R["parcel"], "ReLoop: half arrive by hand at a store. DC: every item is mailed in."),
     ("Receive, inspect, grade", "=(%s-3)*%s" % (R["lab_min"], R["lab_rate"]), "=4*0.35", "ReLoop: staff minutes less the 3 used for packing. DC wage assumed $0.35/min loaded."),
     ("Photograph and list", "=0", "=3*0.35", "ReLoop: the seller's 4 guided photos are the listing."),
     ("Acceptance rate", "=1", "=0.7", "ThredUp accepts roughly 50-80% of a kit; ReLoop only receives items that have already sold."),
     ("Intake cost per ACCEPTED item", "=(B4+B5+B6)/B7", "=(C4+C5+C6)/C7", ""),
     ("Storage until sold", "=0", "=2*0.5", "DC: ~2 months at $0.50 per item-month (ESTIMATE). ReLoop: the garment stays in the seller's wardrobe until it sells."),
     ("Pick, pack", "=3*%s+%s" % (R["lab_rate"], R["pack"]), "=2*0.35+%s" % R["pack"], ""),
     ("Outbound parcel", "=%s" % R["parcel"], "=%s" % R["parcel"], ""),
     ("COST TO SERVE PER UNIT SOLD", "=B8+B9+B10+B11", "=C8+C9+C10+C11", ""),
     ("ReLoop saving vs central DC", "=1-B12/C12", "", "Share of logistics cost avoided by using stores and selling before intake."),
    ]
    for i, rw in enumerate(rows):
        for j, v in enumerate(rw):
            c = cs.cell(row=4 + i, column=1 + j, value=v)
            if j in (1, 2): c.number_format = "0.00"
    cs["B7"].number_format = "0%"; cs["C7"].number_format = "0%"; cs["B13"].number_format = "0%"
    cs.column_dimensions["A"].width = 34; cs.column_dimensions["B"].width = 26; cs.column_dimensions["C"].width = 26; cs.column_dimensions["D"].width = 110

    # ---------- Seller net proceeds vs third parties
    sn = wb.create_sheet("SellerNet")
    sn["A1"] = "What the seller keeps on the same sale, by channel ($). This is the comparison the price-led prompt shows."; sn["A1"].font = Font(bold=True, size=13)
    heads = ["Channel", "Fee rule (2026)", "Sale at $82", "Sale at $63", "Sale at $46", "Sale at $27", "Who prices / verifies / ships"]
    for i, h in enumerate(heads, 1):
        c = sn.cell(row=3, column=i, value=h); c.font = hf; c.fill = hdr
    for j, p in enumerate((82, 63, 46, 27)): sn.cell(row=4, column=3 + j, value=p)
    sn.cell(row=4, column=1, value="Sale price")
    ch = [
     ("ZARA RELOOP", "0% seller fee + dynamic Zara-credit bonus (shown: $4 average) + 10% if paid in credit", "=({p}+4)*1.1", "Zara prices from comps, staff verify, store ships. Seller: 4 photos + one drop-off."),
     ("Zara Pre-Owned today", "0% seller fee", "={p}", "Seller prices (guidance only), self-declared condition, seller packs and ships."),
     ("Vinted", "0% seller fee (buyer pays 5% + $0.70)", "={p}", "Seller prices, ships. No Zara item data."),
     ("Depop", "0% selling fee + 3.3% + $0.45 processing", "={p}*(1-0.033)-0.45", "Seller prices, ships. Owned by eBay since July 2026."),
     ("Mercari", "10% seller fee", "={p}*0.9", "Seller prices, ships."),
     ("eBay", "13.6% final value fee + $0.40", "={p}*(1-0.136)-0.4", "Seller prices, ships."),
     ("Poshmark", "20% ($2.95 under $15)", "=IF({p}<15,{p}-2.95,{p}*0.8)", "Seller prices, ships."),
     ("ThredUp (consignment)", "Payout ~22% of price at $20-50, ~45% at $50-100, less $14.99 kit fee spread over ~10 items", "=IF({p}<50,{p}*0.225,{p}*0.45)-1.5", "ThredUp prices, photographs, ships. Seller mails a bag and waits."),
    ]
    for i, (name, rule, f, who) in enumerate(ch):
        rr = 5 + i
        sn.cell(row=rr, column=1, value=name).font = bold if i == 0 else Font(); sn.cell(row=rr, column=2, value=rule)
        for j, col in enumerate("CDEF"):
            c = sn.cell(row=rr, column=3 + j, value=f.replace("{p}", col + "$4")); c.number_format = "0.00"
        sn.cell(row=rr, column=7, value=who)
    sn.cell(row=14, column=1, value="Fee rules: Zara terms and Vestiaire/COS/StockX pages are primary sources; Depop, Mercari, eBay, Poshmark and ThredUp rates come from 2026 fee trackers - re-check before quoting on a slide. Same sale price assumed on every channel; in practice the verified listing should clear higher and faster.")
    sn.column_dimensions["A"].width = 24; sn.column_dimensions["B"].width = 78; sn.column_dimensions["G"].width = 80
    for col in "CDEF": sn.column_dimensions[col].width = 13

    # ---------- Dynamic credit engine
    ce = wb.create_sheet("CreditEngine")
    ce["A1"] = "Dynamic demand bonus: face value as a function of scarcity"; ce["A1"].font = Font(bold=True, size=13)
    ce["A3"] = "Maximum bonus B_max ($)"; ce["B3"] = 10; ce["B3"].font = inp
    ce["A4"] = "Scarcity at which the maximum is reached (rho_max)"; ce["B4"] = 8; ce["B4"].font = inp
    ce["A5"] = "Minimum scarcity to trigger any bonus"; ce["B5"] = 1; ce["B5"].font = inp
    ce["A6"] = "Guardrail: bonus may not exceed this share of the unit's expected value to Zara"; ce["B6"] = 0.5; ce["B6"].font = inp
    ce["A7"] = "Expected value of one more sold-out-bucket sale to Zara ($)"
    ce["B7"] = "=%s+%s*%s*%s+%s+%s" % (UE("fee", "D"), R["save_rate"], R["save_val"], R["cm"], UE("cl_m", "D"), UE("nb_m", "D")); ce["B7"].number_format = "0.00"
    ce["A8"] = "Guardrail ceiling on bonus cost ($)"; ce["B8"] = "=B6*B7"; ce["B8"].number_format = "0.00"
    ce["A9"] = "Cost of $1 of bonus credit"; ce["B9"] = "=%s" % UE("cfac", "D"); ce["B9"].number_format = "0.000"
    for i, h in enumerate(["Scarcity rho = 14-day expected demand / verified supply", "Formula bonus ($ face)", "After guardrail ($ face)", "Cost to Zara ($)", "Assumed share of sold-out listings"], 1):
        c = ce.cell(row=11, column=i, value=h); c.font = hf; c.fill = hdr
    dist = [(0.5, 0.15), (1, 0.20), (2, 0.25), (3, 0.15), (5, 0.15), (8, 0.07), (12, 0.03)]
    for i, (rho, w) in enumerate(dist):
        rr = 12 + i
        ce.cell(row=rr, column=1, value=rho)
        ce.cell(row=rr, column=2, value="=IF(A%d<$B$5,0,ROUND(MIN($B$3,$B$3*LN(1+A%d)/LN(1+$B$4)),0))" % (rr, rr))
        ce.cell(row=rr, column=3, value="=MIN(B%d,ROUNDDOWN($B$8/$B$9,0))" % rr)
        ce.cell(row=rr, column=4, value="=C%d*$B$9" % rr).number_format = "0.00"
        ce.cell(row=rr, column=5, value=w).number_format = "0%"
    ce["A20"] = "Weighted average bonus ($ face) - compare with the 'bonus_face' assumption"; ce["C20"] = "=SUMPRODUCT(C12:C18,E12:E18)"; ce["C20"].number_format = "0.00"
    ce["A21"] = "Weighted average cost to Zara ($)"; ce["D21"] = "=SUMPRODUCT(D12:D18,E12:E18)"; ce["D21"].number_format = "0.00"
    ce["A23"] = "Time decay: 100% of the bonus if listed within 7 days of the prompt, 50% in days 8-30, 0 after. Paid only if the garment sells inside the 30-day window. Cap $20 of bonus per customer per month."
    ce.column_dimensions["A"].width = 78
    for col in "BCDE": ce.column_dimensions[col].width = 24
    wb.save(path)
    return {"M": M, "U": U}

if __name__ == "__main__":
    out = sys.argv[1]; scn = int(sys.argv[2]) if len(sys.argv) > 2 else 2
    ov = {}
    for a in sys.argv[3:]:
        k, v = a.split("="); ov[k] = float(v)
    import json
    print(json.dumps(build(out, scn, ov)))
