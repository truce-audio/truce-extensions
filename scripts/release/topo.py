#!/usr/bin/env python3
#
# topo.py — emit truce-extensions workspace crates in publish order, one
# per line.
#
# Every workspace member is a candidate except crates that opt out with
# `publish = false`. They are topo-sorted by intra-workspace (non-dev)
# dependencies so each crate's deps are on the registry before it is.
# Today that is truce-lv2 → truce-lv2-meta → truce-format-lv2; the sort
# is here so the next format doesn't mean editing a hard-coded list.
#
# Dependencies on truce's own crates are not members, so they don't
# constrain the order - release.sh separately requires them to be on
# crates.io before it publishes anything.
#
# Invoked by release.sh; runnable standalone to check the order.

import json
import os
import shutil
import subprocess
import sys


def cargo_bin():
    explicit = os.environ.get("CARGO")
    if explicit:
        return explicit
    for cand in ("cargo.exe", "cargo"):
        if shutil.which(cand):
            return cand
    return "cargo"


def publishable_members():
    meta = json.loads(
        subprocess.check_output(
            [cargo_bin(), "metadata", "--format-version", "1", "--no-deps"]
        )
    )
    members = set(meta["workspace_members"])
    return [
        p
        for p in meta["packages"]
        if p["id"] in members and p.get("publish") != []
    ]


def topo_sort(pkgs):
    """Kahn's algorithm, alphabetical tie-break for a deterministic order."""
    names = {p["name"] for p in pkgs}
    incoming = {
        p["name"]: {
            d["name"]
            for d in p["dependencies"]
            if d.get("kind") != "dev" and d["name"] in names and d["name"] != p["name"]
        }
        for p in pkgs
    }

    order = []
    ready = sorted(n for n, deps in incoming.items() if not deps)
    while ready:
        n = ready.pop(0)
        order.append(n)
        for m, deps in incoming.items():
            if n in deps:
                deps.discard(n)
                if not deps and m not in order and m not in ready:
                    ready.append(m)
        ready.sort()

    remaining = [n for n in incoming if n not in order]
    if remaining:
        sys.exit(f"cycle: unresolved={remaining}")
    return order


for name in topo_sort(publishable_members()):
    print(name)
