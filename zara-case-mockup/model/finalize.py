import json, sys, os, subprocess, shutil
from openpyxl import load_workbook
from openpyxl.styles import Font, PatternFill
import build_model as bm
S = sys.argv[1]; d = json.load(open(os.path.join(S, "results.json")))
raw = os.path.join(S, "final_raw.xlsx"); bm.build(raw, 2)
wb = load_workbook(raw)
hdr = PatternFill("solid", fgColor="111111"); hf = Font(bold=True, color="FFFFFF"); bold = Font(bold=True)
sc = wb.create_sheet("Scenarios", 1)
sc["A1"] = "Scenario summary (values pasted from this model at scenario = 1, 2, 3; flip Assumptions!C2 to reproduce)"; sc["A1"].font = Font(bold=True, size=13)
rows = [("Units sold, Year 3", "units", 3, "#,##0"), ("Units sold, 3 years", "units", 4, "#,##0"), ("GMV paid to sellers, Year 3 ($)", "gmv", 3, "#,##0"),
        ("Platform revenue, Year 3 ($)", "prev", 3, "#,##0"), ("Net incremental Zara revenue, Year 3 ($)", "zrev", 3, "#,##0"),
        ("Net incremental Zara revenue, 3 years ($)", "zrev", 4, "#,##0"), ("Variable contribution, Year 3 ($)", "vc", 3, "#,##0"),
        ("Operating contribution, Year 1 ($)", "op", 1, "#,##0"), ("Operating contribution, Year 2 ($)", "op", 2, "#,##0"), ("Operating contribution, Year 3 ($)", "op", 3, "#,##0"),
        ("Capex ($)", "capex", 0, "#,##0"), ("Cumulative cash after Year 3 ($)", "cum", 3, "#,##0"), ("3-year NPV ($)", "npv", 0, "#,##0"),
        ("Zara credit issued, Year 3 ($ face)", "credit", 3, "#,##0"), ("Bonus credit, Year 3 ($ face)", "bonus", 3, "#,##0"),
        ("Items per store per day, Year 3", "perstore", 3, "0.0"), ("Net CO2e avoided, 3 years (t)", "co2", 4, "#,##0")]
for i, h in enumerate(["Metric", "Conservative", "Base (recommended design)", "Upside"], 1):
    c = sc.cell(row=3, column=i, value=h); c.font = hf; c.fill = hdr
for i, (lab, k, idx, fmt) in enumerate(rows):
    sc.cell(row=4 + i, column=1, value=lab)
    for j, n in enumerate(("conservative", "base", "upside")):
        c = sc.cell(row=4 + i, column=2 + j, value=d["scen"][n]["model"][k][idx]); c.number_format = fmt
r0 = 4 + len(rows)
sc.cell(row=r0, column=1, value="Net contribution per unit, Year 3 ($)")
for j, n in enumerate(("conservative", "base", "upside")): sc.cell(row=r0, column=2 + j, value=d["scen"][n]["unit"]["net"][2]).number_format = "0.00"
sc.column_dimensions["A"].width = 48
for col in "BCD": sc.column_dimensions[col].width = 26
se = wb.create_sheet("Sensitivity", 2)
se["A1"] = "One-at-a-time changes to the Base column (values pasted from recalculated runs)"; se["A1"].font = Font(bold=True, size=13)
for i, h in enumerate(["Change", "Net contribution / unit Y3 ($)", "Units sold Y3", "Operating contribution Y3 ($)", "Cumulative cash after Y3 ($)", "3-year NPV ($)", "Net incremental Zara revenue Y3 ($)"], 1):
    c = se.cell(row=3, column=i, value=h); c.font = hf; c.fill = hdr
b = d["scen"]["base"]
allr = [("BASE", {"net_y3": b["unit"]["net"][2], "units_y3": b["model"]["units"][3], "op_y3": b["model"]["op"][3], "cum": b["model"]["cum"][3], "npv": b["model"]["npv"][0], "zrev_y3": b["model"]["zrev"][3]})] + list(d["levers"].items())
names = {"current_policy_fee5_ship4.95_credit50": "Today's policy: fee stays $1 + 5%, shipping $4.95, half take credit",
 "fee_stays_5pct": "Fee stays at $1 + 5% (no verified-service fee)", "ship_subsidised_4.95": "Shipping charged at $4.95 instead of cost",
 "credit_share_50pct": "Only 50% take Zara credit", "incrementality_25pct": "Credit incrementality 25% (base 40%)", "incrementality_55pct": "Credit incrementality 55%",
 "cannibalisation_x1.5": "Cannibalisation 1.5x base", "cannibalisation_x0.5": "Cannibalisation 0.5x base", "conversion_1.5pct": "Prompt conversion 1.5% (base 3%)",
 "conversion_5pct": "Prompt conversion 5%", "parcel_8.50": "Parcel cost $8.50 (list rate), fee unchanged", "parcel_5.50": "Parcel cost $5.50",
 "price_floor_avg_42": "Higher price floor: average sale $42", "dropoff_70pct": "70% drop off in store", "no_demand_bonus": "No dynamic demand bonus (same volume assumed)",
 "labour_9min": "Staff time 9 min per item (base 6)"}
for i, (k, v) in enumerate(allr):
    se.cell(row=4 + i, column=1, value=names.get(k, k)).font = bold if k == "BASE" else Font()
    for j, kk in enumerate(("net_y3", "units_y3", "op_y3", "cum", "npv", "zrev_y3")):
        c = se.cell(row=4 + i, column=2 + j, value=v[kk]); c.number_format = "0.00" if kk == "net_y3" else "#,##0"
se.column_dimensions["A"].width = 66
for col in "BCDEFG": se.column_dimensions[col].width = 26
tmp = os.path.join(S, "ZARA_ReLoop_Financial_Model.xlsx"); wb.save(tmp)
outdir = os.path.join(S, "final"); os.makedirs(outdir, exist_ok=True)
subprocess.run(["soffice", "--headless", "--convert-to", "xlsx", "--outdir", outdir, tmp], check=True, capture_output=True, timeout=180)
shutil.copy(os.path.join(outdir, "ZARA_ReLoop_Financial_Model.xlsx"), "ZARA_ReLoop_Financial_Model.xlsx")
chk = load_workbook("ZARA_ReLoop_Financial_Model.xlsx", data_only=True)
print(chk.sheetnames); errs = 0
for ws in chk:
    for row in ws.iter_rows():
        for c in row:
            if isinstance(c.value, str) and c.value.startswith(("#", "Err:")): errs += 1; print("ERR", ws.title, c.coordinate, c.value)
print("errors:", errs, "| base Y3 op:", chk["Model"]["E28"].value, "| units Y3:", chk["Model"]["E14"].value)
