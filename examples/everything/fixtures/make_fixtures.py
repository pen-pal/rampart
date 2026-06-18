#!/usr/bin/env python3
"""Generate the two captured profiling fixtures the provisioner curls once so
BOTH protobuf profiling formats genuinely hit Rampart's ingest:

  profile.pb.gz       — a gzipped pprof (profile.proto) CPU profile
  otlp-profiles.pb    — an OTLP v1development ExportProfilesServiceRequest

Hand-encoded protobuf (varint + length-delimited) matching exactly the prost
message shapes Rampart's decoders declare (backend/.../pprof.rs and
otlp_profiles.rs). No protobuf library needed — pure stdlib.
"""
import gzip
import struct
import sys
import os


def varint(n):
    out = bytearray()
    while True:
        b = n & 0x7F
        n >>= 7
        if n:
            out.append(b | 0x80)
        else:
            out.append(b)
            return bytes(out)


def tag(field, wire):
    return varint((field << 3) | wire)


def vfield(field, n):          # varint field
    return tag(field, 0) + varint(n)


def lfield(field, payload):    # length-delimited field
    return tag(field, 2) + varint(len(payload)) + payload


def sfield(field, s):          # string field
    return lfield(field, s.encode())


def packed_varints(field, nums):
    body = b"".join(varint(n) for n in nums)
    return lfield(field, body)


# ── pprof (profile.proto) ────────────────────────────────────────────────────
# string_table[0] must be "" by convention; we add frames + the sample-type name.
def build_pprof():
    st = ["", "samples", "count", "cpu", "main", "handler", "db_query", "render"]
    # ValueType sample_type: type=idx("cpu"=3 reused as name? pprof type names
    # come from string_table) — use type=st("samples")=1, unit=st("count")=2,
    # and add a second sample-type "cpu" so parse picks last col's name "cpu".
    sample_type1 = vfield(1, 1) + vfield(2, 2)   # type=1("samples"), unit=2("count")
    sample_type2 = vfield(1, 3) + vfield(2, 2)   # type=3("cpu"),    unit=2("count")
    # Functions: id, name(strindex)
    fns = b""
    fns += lfield(6, b"")  # keep string_table tag separate; placeholder not used
    functions = [
        vfield(1, 1) + vfield(2, 4),  # fn id=1 name="main"
        vfield(1, 2) + vfield(2, 5),  # fn id=2 name="handler"
        vfield(1, 3) + vfield(2, 6),  # fn id=3 name="db_query"
        vfield(1, 4) + vfield(2, 7),  # fn id=4 name="render"
    ]
    # Locations: id + line{function_id}
    def loc(i, fnid):
        line = vfield(1, fnid)            # Line.function_id
        return vfield(1, i) + lfield(4, line)
    locations = [loc(1, 1), loc(2, 2), loc(3, 3), loc(4, 4)]
    # Samples: location_id (leaf-first) + value [samples,count]
    def sample(loc_ids, vals):
        body = packed_varints(1, loc_ids) + packed_varints(2, vals)
        return body
    samples = [
        sample([3, 2, 1], [10, 10]),  # db_query;handler;main
        sample([4, 2, 1], [7, 7]),    # render;handler;main
    ]
    msg = b""
    msg += lfield(1, sample_type1)
    msg += lfield(1, sample_type2)
    for s in samples:
        msg += lfield(2, s)
    for l in locations:
        msg += lfield(4, l)
    for f in functions:
        msg += lfield(5, f)
    for s in st:
        msg += sfield(6, s)
    msg += vfield(10, 1000000000)   # duration_nanos = 1s
    msg += vfield(12, 10000000)     # period = 10ms
    return msg


# ── OTLP v1development ExportProfilesServiceRequest ──────────────────────────
def build_otlp():
    st = ["", "cpu", "main", "handler", "db_query", "render", "service.name", "demo-backend"]
    # functions: name_strindex
    function_table = [
        vfield(1, 2),  # "main"
        vfield(1, 3),  # "handler"
        vfield(1, 4),  # "db_query"
        vfield(1, 5),  # "render"
    ]
    # locations: line{function_index}
    def loc(fn_idx):
        return lfield(3, vfield(1, fn_idx))
    location_table = [loc(0), loc(1), loc(2), loc(3)]
    dictionary = b""
    for l in location_table:
        dictionary += lfield(2, l)
    for f in function_table:
        dictionary += lfield(3, f)
    for s in st:
        dictionary += sfield(5, s)

    # Profile.sample_type: type_strindex -> "cpu" (idx 1)
    sample_type = lfield(1, vfield(1, 1))
    # location_indices: indices into location_table, referenced by samples.
    # sample1 frames db_query;handler;main → location_table idx [2,1,0]
    # sample2 frames render;handler;main   → location_table idx [3,1,0]
    location_indices = packed_varints(3, [2, 1, 0, 3, 1, 0])

    def sample(start, length, vals):
        return vfield(1, start) + vfield(2, length) + packed_varints(3, vals)
    samples = [sample(0, 3, [10]), sample(3, 3, [7])]

    profile = b""
    profile += sample_type
    for s in samples:
        profile += lfield(2, s)
    profile += location_indices
    profile += vfield(5, 1000000000)  # duration_nanos
    profile += vfield(7, 10000000)    # period
    profile += vfield(9, 0)           # default_sample_type_index

    scope_profiles = lfield(2, profile)            # ScopeProfiles.profiles
    # Resource with service.name attribute
    anyval = sfield(1, "demo-backend")
    kv = sfield(1, "service.name") + lfield(2, anyval)
    resource = lfield(1, kv)                        # Resource.attributes
    resource_profiles = lfield(1, resource) + lfield(2, scope_profiles)

    req = b""
    req += lfield(1, resource_profiles)
    req += lfield(2, dictionary)
    return req


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    pprof = build_pprof()
    with gzip.open(os.path.join(here, "profile.pb.gz"), "wb") as f:
        f.write(pprof)
    otlp = build_otlp()
    with open(os.path.join(here, "otlp-profiles.pb"), "wb") as f:
        f.write(otlp)
    print(f"wrote profile.pb.gz ({len(pprof)}b raw pprof, gzipped) and "
          f"otlp-profiles.pb ({len(otlp)}b)")


if __name__ == "__main__":
    main()
