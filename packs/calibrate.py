#!/usr/bin/env python3
"""Calibrate a rulebook against its cases, with the model the rulebook names.

    NEARAI_API_KEY=... python3 packs/calibrate.py packs/grants-digital-art@2.json [--repeat N] [--json OUT]

Each case in `packs/cases/<pack file name>` names the decision the rulebook
must reach. The script asks Sage exactly what the judge asks (one typed
decision request per text, one yes/no question per rule), applies the same
thresholds in basis points, and prints every rule's P(yes) beside the
decision. With --repeat N it asks each case N times and reports whether every
probability came back identical, compared as the exact numbers Sage returned.
With --json OUT it also writes every case's numbers to OUT.

Exit status: 0 when every case reaches its expected decision.
"""
import json
import math
import os
import sys
import urllib.request

URL = "https://cloud-api.near.ai/v1/systemone"
ORDER = {"pass": 0, "escalate": 1, "block": 2}


def ask(key, pack, text):
    questions = {r["id"]: {"type": "noul", "instructions": r["question"]} for r in pack["rules"]}
    body = json.dumps({"model": pack["model"], "state": text, "questions": questions}).encode()
    request = urllib.request.Request(
        URL,
        data=body,
        headers={
            "Authorization": f"Bearer {key}",
            "Content-Type": "application/json",
            "Accept": "application/json",
            "Accept-Encoding": "identity",
            "x-no-aliasing": "true",
        },
    )
    with urllib.request.urlopen(request, timeout=120) as reply:
        answers = json.load(reply)["answers"]
    return {rule: answer["noul"] for rule, answer in answers.items()}


def decide(pack, bp):
    decision = "pass"
    fired = []
    for rule in pack["rules"]:
        if bp[rule["id"]] >= rule["threshold"] * 100:
            fired.append(rule["id"])
            if ORDER[rule["action"]] > ORDER[decision]:
                decision = rule["action"]
    return decision, fired


def main():
    key = os.environ.get("NEARAI_API_KEY", "").strip()
    if not key:
        sys.exit("set NEARAI_API_KEY")
    pack_path = sys.argv[1]
    repeat = int(sys.argv[sys.argv.index("--repeat") + 1]) if "--repeat" in sys.argv else 1
    out = sys.argv[sys.argv.index("--json") + 1] if "--json" in sys.argv else None
    record = []
    pack = json.load(open(pack_path))
    cases_path = os.path.join(os.path.dirname(pack_path), "cases", os.path.basename(pack_path).split("@")[0] + ".json")
    cases = json.load(open(cases_path))

    rules = [r["id"] for r in pack["rules"]]
    print(f"{pack['id']}@{pack['version']}  model {pack['model']}  {len(cases)} cases  x{repeat}")
    print("  " + "  ".join(f"{r[:18]:>18}" for r in rules) + "  decision  expected")
    failures = 0
    unstable = 0
    for case in cases:
        runs = [ask(key, pack, case["text"]) for _ in range(repeat)]
        if any(run != runs[0] for run in runs[1:]):
            unstable += 1
        # Half away from zero, as the judge rounds (Rust's f64::round).
        bp = {rule: math.floor(p * 10_000 + 0.5) for rule, p in runs[0].items()}
        decision, _ = decide(pack, bp)
        ok = decision == case["expect"]
        failures += not ok
        cells = "  ".join(f"{bp[r] / 100:>17.2f}%" for r in rules)
        same = "" if repeat == 1 else ("  same x%d" % repeat if all(run == runs[0] for run in runs) else "  DIFFERS")
        print(f"{'ok' if ok else '!!'} {cells}  {decision:>8}  {case['expect']:>8}  {case['name']}{same}")
        record.append({
            "name": case["name"],
            "text": case["text"],
            "expect": case["expect"],
            "decision": decision,
            "ok": ok,
            "bp": bp,
            "identical": all(run == runs[0] for run in runs),
        })
    print(f"{len(cases) - failures}/{len(cases)} cases reach their expected decision")
    if repeat > 1:
        print(f"{len(cases) - unstable}/{len(cases)} cases answered identically on all {repeat} asks")
    if out:
        with open(out, "w") as f:
            json.dump({
                "pack": f"{pack['id']}@{pack['version']}",
                "model": pack["model"],
                "repeat": repeat,
                "rules": [{"id": r["id"], "threshold": r["threshold"], "action": r["action"]} for r in pack["rules"]],
                "cases": record,
            }, f, indent=1, ensure_ascii=False)
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
