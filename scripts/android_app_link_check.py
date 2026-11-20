#!/usr/bin/env python3
"""Check the verified setup App Link in an APK manifest dump (`aapt2 dump xmltree`).

Usage: aapt2 dump xmltree --file AndroidManifest.xml app.apk | android_app_link_check.py <host>

Passes only when MainActivity has an intent filter that is exactly the setup App Link:
autoVerify, the VIEW action, the DEFAULT and BROWSABLE categories, and data of exactly
scheme https, host <host> and path /setup.

Every https filter of MainActivity, and of every activity-alias that targets it, is looked at.
It fails the check when it reaches more of <host> than exact paths:
- pathPrefix, pathPattern, pathSuffix, pathAdvancedPattern, or no path at all (which matches
  every path), or a port;
- a wildcard host that covers <host> (Android matches a host that starts with * against every
  host that ends with the rest), whatever paths it names;
- no host at all (Android then matches every https link, whatever paths it names);
as does a setup filter with any other data. Only a filter's own <data> elements count; a
uri-relative-filter-group inside an https filter for <host> fails the check (Android before 15
ignores the group, so its paths would not limit the filter there). Other filters for <host> with
exact paths of their own stay allowed. A setup filter on an alias does not stand in for the one
on MainActivity.
"""
from __future__ import annotations

import re
import sys

VIEW = "android.intent.action.VIEW"
CATEGORIES = {"android.intent.category.DEFAULT", "android.intent.category.BROWSABLE"}
LOOSE = ("pathPrefix", "pathPattern", "pathSuffix", "pathAdvancedPattern", "port")
EXACT_DATA = {"scheme", "host", "path"}
MAIN = "io.wolftown.kaiku.ui.MainActivity"


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


def host_matches(pattern: str, host: str) -> bool:
    """Android's host matching: the host itself, or a leading * for every host that ends with the rest."""
    if pattern.startswith("*"):
        return host.endswith(pattern[1:])
    return pattern == host


def full_name(name: str, package: str) -> str:
    """A component name as Android resolves it: ".X" and a name without a dot are in the manifest's package."""
    if name.startswith("."):
        return package + name
    if "." not in name:
        return f"{package}.{name}" if package else name
    return name


def components(nodes: list[dict]):
    """MainActivity (by its full class name) and every activity-alias that targets it, as (element, is_alias)."""
    package = next((m["attrs"].get("package", "") for m in walk(nodes, "manifest")), "")
    for act in walk(nodes, "activity"):
        if full_name(act["attrs"].get("name", ""), package) == MAIN:
            yield act, False
    for alias in walk(nodes, "activity-alias"):
        if full_name(alias["attrs"].get("targetActivity", ""), package) == MAIN:
            yield alias, True


def problems(tree: str, host: str) -> list[str]:
    """Everything wrong with the setup App Link for host; empty when it is exactly right."""
    found = False
    wrong: list[str] = []
    for comp, alias in components(parse(tree)):
        where = " (on an alias of MainActivity)" if alias else ""
        for f in walk(comp["children"], "intent-filter"):
            # Only the filter's own data: data inside a group is not read by every Android version.
            data = [d["attrs"] for d in f["children"] if d["tag"] == "data"]
            if not any(d.get("scheme") == "https" for d in data):
                continue
            named = [d["host"] for d in data if "host" in d]
            if not named:
                # No host at all: Android matches every https link, whatever paths are named.
                wrong.append("https filter without a host" + where)
                continue
            wild = sorted({h for h in named if h.startswith("*") and host_matches(h, host)})
            if wild:
                wrong.append(f"https filter with a wildcard host that covers {host}: {', '.join(wild)}" + where)
                continue
            if host not in named:
                continue
            if any(True for _ in walk(f["children"], "uri-relative-filter-group")):
                wrong.append(f"https filter for {host} uses uri-relative-filter-group" + where)
                continue
            loose = sorted({k for d in data for k in d if k in LOOSE})
            if loose:
                wrong.append(f"https filter for {host} uses {', '.join(loose)}" + where)
                continue
            paths = {d["path"] for d in data if "path" in d}
            if not paths:
                # No path at all: Android matches every path on the host.
                wrong.append(f"https filter for {host} without a path" + where)
                continue
            if "/setup" not in paths:
                continue
            actions = {a["attrs"].get("name") for a in walk(f["children"], "action")}
            categories = {c["attrs"].get("name") for c in walk(f["children"], "category")}
            schemes = {d["scheme"] for d in data if "scheme" in d}
            hosts = set(named)
            extra = sorted({k for d in data for k in d if k not in EXACT_DATA})
            verify = f["attrs"].get("autoVerify", "").lower() in ("true", "0xffffffff", "-1")
            if not verify:
                wrong.append("setup filter without autoVerify" + where)
            elif VIEW not in actions:
                wrong.append("setup filter without the VIEW action" + where)
            elif not CATEGORIES <= categories:
                wrong.append("setup filter without the DEFAULT and BROWSABLE categories" + where)
            elif schemes != {"https"} or hosts != {host} or paths != {"/setup"} or extra:
                wrong.append("setup filter with other data than https://" + host + "/setup" + where)
            elif not alias:
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
