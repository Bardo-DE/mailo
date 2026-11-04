"""Compare scripts/boundary.toml with `cargo metadata` (JSON on stdin). Exit 1 on any difference.

Run through scripts/check-boundary.sh; see the header of boundary.toml for what the table means.
"""
import fnmatch
import json
import subprocess
import sys
import tomllib

table = tomllib.load(open(sys.argv[1], "rb"))
meta = json.load(sys.stdin)

pkgs = {p["id"]: p for p in meta["packages"]}
members = {i: pkgs[i] for i in meta["workspace_members"]}
ws = {p["name"] for p in members.values()}
errors = []

# Declared dependencies of each member: the Cargo.toml lists, optional and target-specific ones
# included. Self edges (a dev-dependency that turns on the crate's own feature) are not edges.
declared = {}  # name -> {"deps": set, "dev": set, "all": set of every declared name}
for p in members.values():
    d = {"deps": set(), "dev": set(), "all": set()}
    for dep in p["dependencies"]:
        if dep["name"] == p["name"]:
            continue
        d["all"].add(dep["name"])
        if dep["name"] in ws:
            d["dev" if dep["kind"] == "dev" else "deps"].add(dep["name"])
    d["dev"] -= d["deps"]
    declared[p["name"]] = d

# The table: every workspace crate, no more, no fewer; every edge, no more, no fewer.
crates = table.get("crates", {})
for name in sorted(ws - crates.keys()):
    errors.append(f"{name}: a workspace crate with no [crates.{name}] in scripts/boundary.toml")
for name in sorted(crates.keys() - ws):
    errors.append(f"{name}: in scripts/boundary.toml but not a workspace crate: remove [crates.{name}]")
for name in sorted(ws & crates.keys()):
    for kind in ("deps", "dev"):
        want = set(crates[name].get(kind, []))
        have = declared[name][kind]
        for dep in sorted(want - ws):
            errors.append(f"{name}: {kind} lists {dep}, which is not a workspace crate")
        for dep in sorted(have - want):
            where = "dev-dependency" if kind == "dev" else "dependency"
            errors.append(f"{name}: new {where} on {dep}: add it to {kind} of [crates.{name}] in scripts/boundary.toml, if it is meant")
        for dep in sorted((want & ws) - have):
            errors.append(f"{name}: scripts/boundary.toml lists {kind} {dep}, which Cargo.toml no longer has: remove it")
    stray = set(crates[name]) - {"deps", "dev"}
    if stray:
        errors.append(f"{name}: unknown keys {sorted(stray)} in [crates.{name}]")

# Reachability, as `cargo tree` follows it: every kind of edge, and the features that crate alone
# turns on (a workspace-wide `cargo metadata` unifies them, which would charge `mail-ui-model` with
# the dioxus that `mail-ui` asks `ds-settings` for).
def tree(*args):
    return subprocess.run(["cargo", "tree", "-e", "normal,build,dev", *args],
                          capture_output=True, text=True)


reached = {}  # crate -> names of every package it reaches


def reach(name):
    if name not in reached:
        out = tree("-p", name, "--prefix", "none", "--format", "{p}")
        if out.returncode != 0:
            errors.append(f"cargo tree -p {name} failed: {out.stderr.strip().splitlines()[-1:]}")
        reached[name] = {line.split()[0] for line in out.stdout.splitlines() if line.strip()} - {name}
    return reached[name]


for group in table.get("forbid", []):
    for name in group["crates"]:
        if name not in ws:
            errors.append(f"[[forbid]] names {name}, which is not a workspace crate")
            continue
        for target in sorted(set(group["reach_none"]) & reach(name)):
            if target == name:
                continue
            why = tree("-p", name, "-i", target, "--prefix", "none", "--format", "{p}").stdout
            via = " <- ".join(dict.fromkeys(l.split()[0] for l in why.splitlines() if l.strip()))
            errors.append(f"{name} reaches {target}, which it may not (reverse chain: {via})")

for group in table.get("forbid_direct", []):
    for name in group["crates"]:
        if name not in ws:
            errors.append(f"[[forbid_direct]] names {name}, which is not a workspace crate")
            continue
        for dep in sorted(declared[name]["all"]):
            for pat in group["names"]:
                if fnmatch.fnmatchcase(dep, pat):
                    errors.append(f"{name} declares {dep} (matches {pat}), which it may not")

if errors:
    print("dependency boundary broken (scripts/boundary.toml):")
    for e in errors:
        print("  " + e)
    sys.exit(1)

edges = sum(len(declared[n]["deps"]) + len(declared[n]["dev"]) for n in ws)
print(f"dependency boundary holds: {len(ws)} crates, {edges} workspace edges match scripts/boundary.toml; "
      f"{len(table.get('forbid', []))} reach rules and {len(table.get('forbid_direct', []))} direct rules clear")
