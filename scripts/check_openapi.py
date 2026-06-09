#!/usr/bin/env python3
"""Derive the registered route-path set from the axum source and diff it
against docs/openapi.yaml.

We do NOT build the router (axum 0.8 won't hand us the route table cheaply).
Instead we model the small, well-understood composition the codebase uses:

  * Each `routes/*.rs` exposes one or more `pub fn <name>() -> Router` that
    chain `.route("<path>", ...)` calls. We collect the literal paths per fn.
  * `lib.rs` and `routes/mod.rs` compose those fns with `.nest("<prefix>",
    EXPR)` and `.merge(EXPR)`, where EXPR is a router-returning expression
    (e.g. `monitors::router()`), possibly itself `.merge(...)`-chained.

We walk that composition, accumulating the nest-prefix, to produce the set of
full paths the server registers. Placeholders ({id}/{slug}/{token}/...) are
normalised to `{}` on both sides so the diff is param-name agnostic.

Exit codes:
  0  every registered route is present in the spec
  1  one or more registered routes are missing from the spec
  2  internal error (couldn't parse / locate inputs)
"""

from __future__ import annotations

import argparse
import os
import re
import sys

# Routes that are intentionally NOT in the public OpenAPI spec. Keep this
# list tiny and justified — every entry is a deliberate choice, not a
# convenient way to silence the check. Paths are post-normalisation
# (placeholders collapsed to {}).
ALLOWLIST = {
    # Operational endpoints, not part of the documented client-facing API
    # surface. /healthz, /readyz, /metrics, /openapi.* ARE documented today,
    # so they are not listed here — only add a path when it is genuinely
    # internal and should stay undocumented.
}


def normalise(path: str) -> str:
    """Collapse `{anything}` to `{}` and strip a trailing slash (but keep a
    bare root `/`)."""
    path = re.sub(r"\{[^}]*\}", "{}", path)
    if len(path) > 1 and path.endswith("/"):
        path = path[:-1]
    return path


def join(prefix: str, path: str) -> str:
    """Join a nest prefix with an inner route path the way axum does."""
    if path == "/":
        # `.route("/", ...)` inside a `.nest("/foo", ...)` registers `/foo`.
        joined = prefix if prefix else "/"
    else:
        joined = f"{prefix}{path}"
    # Collapse any accidental double slashes.
    joined = re.sub(r"/{2,}", "/", joined)
    return joined


# ── Parse each module fn → list of literal route paths ──────────────────────

# Matches `pub fn name() -> Router<...> {`  (or `-> Router {`)
FN_RE = re.compile(r"pub\s+fn\s+(\w+)\s*\([^)]*\)\s*->\s*Router")
# Matches a `.route("<path>"` — `<path>` may sit on the same or the next line.
ROUTE_RE = re.compile(r"\.route(?:_service)?\s*\(\s*\n?\s*\"([^\"]+)\"")


def parse_module(text: str) -> dict[str, list[str]]:
    """Return {fn_name: [literal paths]} for one module's source.

    We slice the source at each `pub fn ... -> Router` boundary and scan the
    body up to the next such boundary for `.route(...)` literals. This is
    deliberately simple: the route fns in this codebase are flat builder
    chains, so a per-fn text window captures their paths reliably.
    """
    out: dict[str, list[str]] = {}
    matches = list(FN_RE.finditer(text))
    for i, m in enumerate(matches):
        name = m.group(1)
        start = m.end()
        end = matches[i + 1].start() if i + 1 < len(matches) else len(text)
        body = text[start:end]
        paths = ROUTE_RE.findall(body)
        out[name] = paths
    return out


def load_modules(routes_dir: str) -> dict[str, dict[str, list[str]]]:
    """{module_stem: {fn: [paths]}} for every routes/*.rs except mod.rs."""
    mods: dict[str, dict[str, list[str]]] = {}
    for fn in sorted(os.listdir(routes_dir)):
        if not fn.endswith(".rs") or fn == "mod.rs":
            continue
        stem = fn[:-3]
        with open(os.path.join(routes_dir, fn), encoding="utf-8") as fh:
            mods[stem] = parse_module(fh.read())
    return mods


# ── Resolve a router-returning expression to its set of paths ───────────────

# A single `module::fn()` call.
CALL_RE = re.compile(r"(\w+)::(\w+)\s*\(\s*\)")


def expr_paths(
    expr: str,
    mods: dict[str, dict[str, list[str]]],
    unresolved: list[str],
) -> list[str]:
    """All literal route paths produced by a router expression.

    The expression is a chain like
        monitors::router().merge(notifications::monitor_attach_router())
    We simply union the paths of every `module::fn()` call it mentions —
    `.merge()` is a flat union and any `.route_layer(...)` / `.with_state(...)`
    calls carry no paths, so collecting every call is exactly right.
    """
    paths: list[str] = []
    for mod_name, fn_name in CALL_RE.findall(expr):
        modmap = mods.get(mod_name)
        if modmap is None:
            continue  # not a route module (e.g. middleware helper) — skip
        if fn_name not in modmap:
            unresolved.append(f"{mod_name}::{fn_name}")
            continue
        paths.extend(modmap[fn_name])
    return paths


# Matches `.nest("<prefix>", <EXPR>)` capturing balanced-ish args. We rely on
# the fact that nest args here never contain a `)` outside their own calls; to
# be safe we match up to the matching close by scanning manually.
NEST_OPEN_RE = re.compile(r"\.nest\s*\(\s*\"([^\"]+)\"\s*,")


def find_nests(text: str) -> list[tuple[str, str]]:
    """Return [(prefix, expr_source)] for every `.nest("p", EXPR)` in text,
    handling multi-line EXPR by balancing parentheses from the comma."""
    out: list[tuple[str, str]] = []
    for m in NEST_OPEN_RE.finditer(text):
        prefix = m.group(1)
        i = m.end()
        depth = 1  # we are inside the .nest( … ) call
        start = i
        while i < len(text) and depth > 0:
            c = text[i]
            if c == "(":
                depth += 1
            elif c == ")":
                depth -= 1
            i += 1
        expr = text[start : i - 1]
        out.append((prefix, expr))
    return out


def collect_registered(
    routes_dir: str, lib_rs: str
) -> tuple[set[str], list[str]]:
    """Build the normalised set of full registered paths.

    Strategy: read mod.rs + lib.rs, find every `.nest("prefix", EXPR)`, and
    for each, prefix-join the paths from EXPR. Nests can be layered (mod.rs
    nests under a bare prefix, lib.rs nests *that* under `/v1`). We resolve
    the two layers explicitly because there are exactly two mount points
    that re-nest: lib.rs mounts `/v1`, `/push`, and root-level merges.
    """
    mods = load_modules(routes_dir)
    unresolved: list[str] = []

    with open(os.path.join(routes_dir, "mod.rs"), encoding="utf-8") as fh:
        mod_src = fh.read()
    with open(lib_rs, encoding="utf-8") as fh:
        lib_src = fh.read()

    # mod.rs assembles the /v1 subtree (without the /v1 prefix). Every path it
    # produces is mounted under /v1 by lib.rs. We collect mod.rs's nests with
    # their prefixes, then bolt /v1 on.
    v1_paths: set[str] = set()
    for prefix, expr in find_nests(mod_src):
        for p in expr_paths(expr, mods, unresolved):
            v1_paths.add(join(prefix, p))

    # mod.rs also `.merge()`es some routers directly (no nest prefix) into the
    # /v1 tree — e.g. subscribers::admin_router(), subscribers::settings_router().
    # Those carry their own absolute paths. Find `.merge(EXPR)` at mod scope.
    merged_in_mod = collect_bare_merges(mod_src, mods, unresolved, exclude_nested=True)
    for p in merged_in_mod:
        v1_paths.add(join("", p))

    registered: set[str] = set()
    for p in v1_paths:
        registered.add(join("/v1", p))

    # lib.rs mounts root-level routers + /push, plus re-nests the /v1 tree
    # (already handled above). Walk lib.rs's nests, but skip the `/v1` one
    # since its inner expr is just the composed routers we already expanded.
    for prefix, expr in find_nests(lib_src):
        if prefix == "/v1":
            continue
        for p in expr_paths(expr, mods, unresolved):
            registered.add(join(prefix, p))

    # Root-level `.merge(EXPR)` in lib.rs — health, openapi (public_root).
    # public_root() lives in mod.rs and returns openapi::router(); resolve the
    # indirection by also scanning mod.rs fns whose bodies are pure delegates.
    for p in collect_root_merges(lib_src, mod_src, mods, unresolved):
        registered.add(join("", p))

    normalised = {normalise(p) for p in registered}
    return normalised, sorted(set(unresolved))


MERGE_OPEN_RE = re.compile(r"\.merge\s*\(")


def _balanced_arg(text: str, open_idx: int) -> tuple[str, int]:
    """Given index just after a `(`, return (inner_text, idx_after_close)."""
    depth = 1
    i = open_idx
    start = i
    while i < len(text) and depth > 0:
        c = text[i]
        if c == "(":
            depth += 1
        elif c == ")":
            depth -= 1
        i += 1
    return text[start : i - 1], i


def collect_bare_merges(
    text: str,
    mods: dict[str, dict[str, list[str]]],
    unresolved: list[str],
    exclude_nested: bool,
) -> list[str]:
    """Paths from `.merge(EXPR)` calls. When exclude_nested is True we skip
    merges that appear *inside* a `.nest(...)` arg (those are already counted
    by the nest walk)."""
    nest_spans: list[tuple[int, int]] = []
    if exclude_nested:
        for m in NEST_OPEN_RE.finditer(text):
            _, after = _balanced_arg(text, m.end())
            nest_spans.append((m.start(), after))

    def inside_nest(idx: int) -> bool:
        return any(a <= idx < b for a, b in nest_spans)

    paths: list[str] = []
    for m in MERGE_OPEN_RE.finditer(text):
        if exclude_nested and inside_nest(m.start()):
            continue
        expr, _ = _balanced_arg(text, m.end())
        paths.extend(expr_paths(expr, mods, unresolved))
    return paths


def collect_root_merges(
    lib_src: str,
    mod_src: str,
    mods: dict[str, dict[str, list[str]]],
    unresolved: list[str],
) -> list[str]:
    """Root-level merges in lib.rs (health::router(), routes::public_root()).
    public_root() is a delegate in mod.rs returning openapi::router(); resolve
    it by inlining the paths of any fn it calls."""
    # Resolve public_root() (and similar delegating fns) defined in mod.rs.
    mod_fn_paths: dict[str, list[str]] = {}
    for fn_match in re.finditer(r"pub\s+fn\s+(\w+)\s*\([^)]*\)\s*->\s*Router", mod_src):
        name = fn_match.group(1)
        start = fn_match.end()
        # body until next blank-line-delimited fn end — grab a generous window
        body = mod_src[start : start + 4000]
        # only treat as a delegate if it doesn't itself build nests/routes
        calls = [
            f"{mn}::{fnm}"
            for mn, fnm in CALL_RE.findall(body[: body.find("\n\n") if "\n\n" in body else len(body)])
        ]
        # collect paths from delegated module calls
        dp: list[str] = []
        seg = body[: body.find("}\n") if "}\n" in body else len(body)]
        dp.extend(expr_paths(seg, mods, unresolved))
        if dp:
            mod_fn_paths[name] = dp

    paths: list[str] = []
    for m in MERGE_OPEN_RE.finditer(lib_src):
        expr, _ = _balanced_arg(lib_src, m.end())
        # routes::public_root() → resolve via mod_fn_paths
        for fn in re.findall(r"routes::(\w+)\s*\(\s*\)", expr):
            if fn in mod_fn_paths:
                paths.extend(mod_fn_paths[fn])
        # routes::health::router() style direct module calls
        paths.extend(expr_paths(expr, mods, unresolved))
    return paths


# ── Parse the spec's documented paths ───────────────────────────────────────

SPEC_PATH_RE = re.compile(r"^  (/\S*):\s*$")


def load_spec_paths(spec_file: str) -> set[str]:
    """The `paths:` keys from openapi.yaml (two-space indented top-level keys
    under `paths:`). Normalised the same way as registered paths."""
    out: set[str] = set()
    in_paths = False
    with open(spec_file, encoding="utf-8") as fh:
        for line in fh:
            if re.match(r"^paths:\s*$", line):
                in_paths = True
                continue
            if in_paths and re.match(r"^\S", line):
                # dedented back to a top-level key → paths block ended
                break
            if not in_paths:
                continue
            m = SPEC_PATH_RE.match(line.rstrip("\n"))
            if m:
                out.add(normalise(m.group(1)))
    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--routes-dir", required=True)
    ap.add_argument("--lib-rs", required=True)
    ap.add_argument("--spec", required=True)
    args = ap.parse_args()

    registered, unresolved = collect_registered(args.routes_dir, args.lib_rs)
    documented = load_spec_paths(args.spec)

    allow = {normalise(p) for p in ALLOWLIST}
    registered_effective = registered - allow

    missing = sorted(registered_effective - documented)
    extra = sorted(documented - registered - allow)

    print(f"check-openapi: {len(registered)} registered route paths, "
          f"{len(documented)} documented in spec.")
    if unresolved:
        print("check-openapi: WARNING — could not resolve these router fns "
              "(parser may need updating): " + ", ".join(unresolved),
              file=sys.stderr)

    if extra:
        print("\ncheck-openapi: NOTE — paths in spec not matched to a "
              "registered route (informational only):")
        for p in extra:
            print(f"  - {p}")

    if missing:
        print("\ncheck-openapi: FAIL — these registered routes are MISSING "
              "from docs/openapi.yaml:")
        for p in missing:
            print(f"  + {p}")
        print("\nAdd a `paths:` entry for each, or (if genuinely internal) "
              "add it to ALLOWLIST in scripts/check_openapi.py.")
        return 1

    print("check-openapi: OK — every registered route is documented.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
