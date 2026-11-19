#!/usr/bin/env python3
"""Check the verified setup App Link in an APK manifest dump (`aapt2 dump xmltree`).

Usage: aapt2 dump xmltree --file AndroidManifest.xml app.apk | android_app_link_check.py <host>

Passes only when MainActivity has an intent filter that is exactly the setup App Link:
autoVerify, the VIEW action, the DEFAULT and BROWSABLE categories, and data of exactly
scheme https, host <host> and path /setup. Any https filter of MainActivity for <host> that
matches more than exact paths (pathPrefix, pathPattern, pathSuffix, pathAdvancedPattern) or
names a port fails the check, as does a setup filter with any other data.
"""
from __future__ import annotations

import re
import sys

VIEW = "android.intent.action.VIEW"
CATEGORIES = {"android.intent.category.DEFAULT", "android.intent.category.BROWSABLE"}
LOOSE = ("pathPrefix", "pathPattern", "pathSuffix", "pathAdvancedPattern", "port")
EXACT_DATA = {"scheme", "host", "path"}


def parse(tree: str) -> list[dict]:
    """aapt2 xmltree: "E: <tag>" elements and "A: <ns>:<name>(<id>)=<value>" attributes, nested by indent."""
    nodes: list[dict] = []
    stack: list[dict] = []
    for line in tree.splitlines():
        m = re.match(r"^(\s*)(E|A): (.*)$", line)
        if not m:
            continue
        depth, kind, rest = len(m.group(1)), m.group(2), m.group(3)
        if kind == "E":
            node = {"tag": rest.split(" ")[0], "attrs": {}, "children": [], "depth": depth}
            while stack and stack[-1]["depth"] >= depth:
                stack.pop()
            (stack[-1]["children"] if stack else nodes).append(node)
            stack.append(node)
        elif stack:
            a = re.match(r"^(?:\S+:)?([A-Za-z]+)\([^)]*\)=(.*)$", rest) or re.match(r"^(?:\S+:)?([A-Za-z]+)=(.*)$", rest)
            if a:
                v = a.group(2)
                raw = re.search(r"\(Raw: \"([^\"]*)\"\)", v)
                v = raw.group(1) if raw else v.strip().strip('"')
                stack[-1]["attrs"][a.group(1)] = v
    return nodes


def walk(nodes: list[dict], tag: str):
    for n in nodes:
        if n["tag"] == tag:
            yield n
        yield from walk(n["children"], tag)


def problems(tree: str, host: str) -> list[str]:
    """Everything wrong with the setup App Link for host; empty when it is exactly right."""
    found = False
    wrong: list[str] = []
    for act in walk(parse(tree), "activity"):
        if not act["attrs"].get("name", "").endswith(".ui.MainActivity"):
            continue
        for f in walk(act["children"], "intent-filter"):
            data = [d["attrs"] for d in walk(f["children"], "data")]
            if not any(d.get("host") == host for d in data) or not any(d.get("scheme") == "https" for d in data):
                continue
            loose = sorted({k for d in data for k in d if k in LOOSE})
            if loose:
                wrong.append(f"https filter for {host} uses {', '.join(loose)}")
                continue
            paths = {d["path"] for d in data if "path" in d}
            if "/setup" not in paths:
                continue
            actions = {a["attrs"].get("name") for a in walk(f["children"], "action")}
            categories = {c["attrs"].get("name") for c in walk(f["children"], "category")}
            schemes = {d["scheme"] for d in data if "scheme" in d}
            hosts = {d["host"] for d in data if "host" in d}
            extra = sorted({k for d in data for k in d if k not in EXACT_DATA})
            verify = f["attrs"].get("autoVerify", "").lower() in ("true", "0xffffffff", "-1")
            if not verify:
                wrong.append("setup filter without autoVerify")
            elif VIEW not in actions:
                wrong.append("setup filter without the VIEW action")
            elif not CATEGORIES <= categories:
                wrong.append("setup filter without the DEFAULT and BROWSABLE categories")
            elif schemes != {"https"} or hosts != {host} or paths != {"/setup"} or extra:
                wrong.append("setup filter with other data than https://" + host + "/setup")
            else:
                found = True
    if not found and not wrong:
        wrong.append(f"no verified App Link filter for https://{host}/setup on MainActivity")
    return wrong


def main(argv: list[str]) -> int:
    if len(argv) != 2 or not argv[1]:
        print("usage: android_app_link_check.py <host>  (manifest xmltree on stdin)", file=sys.stderr)
        return 2
    found = problems(sys.stdin.read(), argv[1])
    for p in found:
        print(f"android_app_link_check: {p}", file=sys.stderr)
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
