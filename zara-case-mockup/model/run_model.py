"""Recalculate the model with LibreOffice for each scenario and for one-at-a-time levers; write results.json."""
import json, subprocess, sys, os
from openpyxl import load_workbook
import build_model as bm
S = sys.argv[1]
def run(tag, scn=2, over=None):
    raw = os.path.join(S, tag + "_raw.xlsx"); outdir = os.path.join(S, "calc_" + tag); os.makedirs(outdir, exist_ok=True)
    idx = bm.build(raw, scn, over)
    subprocess.run(["soffice", "--headless", "--convert-to", "xlsx", "--outdir", outdir, raw], check=True, capture_output=True, timeout=180)
    wb = load_workbook(os.path.join(outdir, tag + "_raw.xlsx"), data_only=True)
    m, u = wb["Model"], wb["UnitEconomics"]
    res = {"model": {}, "unit": {}}
    for k, r in idx["M"].items(): res["model"][k] = [m.cell(row=r, column=c).value for c in range(2, 7)]
    for k, r in idx["U"].items(): res["unit"][k] = [u.cell(row=r, column=c).value for c in range(2, 5)]
    res["capex"] = [[c.value for c in row] for row in wb["Capex"].iter_rows(min_row=4, max_col=3) if row[0].value]
    res["cts"] = [[c.value for c in row] for row in wb["CostToServe"].iter_rows(min_row=4, max_row=13, max_col=3)]
    res["seller"] = [[c.value for c in row] for row in wb["SellerNet"].iter_rows(min_row=4, max_row=12, max_col=6)]
    res["credit"] = {"B7": wb["CreditEngine"]["B7"].value, "B8": wb["CreditEngine"]["B8"].value, "B9": wb["CreditEngine"]["B9"].value,
                     "rows": [[c.value for c in row] for row in wb["CreditEngine"].iter_rows(min_row=12, max_row=18, max_col=5)],
                     "avg_face": wb["CreditEngine"]["C20"].value, "avg_cost": wb["CreditEngine"]["D21"].value}
    return res
out = {"scen": {n: run(n, i) for i, n in ((1, "conservative"), (2, "base"), (3, "upside"))}}
levers = {
 "current_policy_fee5_ship4.95_credit50": {"fee2": 0.05, "fee3": 0.05, "ship_fee": 4.95, "cred_share": 0.50},
 "fee_stays_5pct": {"fee2": 0.05, "fee3": 0.05},
 "ship_subsidised_4.95": {"ship_fee": 4.95},
 "credit_share_50pct": {"cred_share": 0.50},
 "incrementality_25pct": {"incr": 0.25},
 "incrementality_55pct": {"incr": 0.55},
 "cannibalisation_x1.5": {"cA": 0.375, "cB": 0.90, "cC": 0.15},
 "cannibalisation_x0.5": {"cA": 0.125, "cB": 0.30, "cC": 0.05},
 "conversion_1.5pct": {"conv": 0.015},
 "conversion_5pct": {"conv": 0.05},
 "parcel_8.50": {"parcel": 8.50},
 "parcel_5.50": {"parcel": 5.50},
 "price_floor_avg_42": {"price": 42.0},
 "dropoff_70pct": {"drop": 0.70},
 "no_demand_bonus": {"bonus_face": 0.0},
 "labour_9min": {"lab_min": 9.0},
}
out["levers"] = {}
for k, ov in levers.items():
    r = run("lev_" + k.replace(".", "_"), 2, ov)
    out["levers"][k] = {"net_y3": r["unit"]["net"][2], "units_y3": r["model"]["units"][3], "op_y3": r["model"]["op"][3], "cum": r["model"]["cum"][3], "npv": r["model"]["npv"][0], "zrev_y3": r["model"]["zrev"][3]}
json.dump(out, open(os.path.join(S, "results.json"), "w"), indent=1)
