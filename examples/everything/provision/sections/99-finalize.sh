# 99-finalize.sh — curl the captured profiling fixtures once so BOTH profile
# formats genuinely hit (folded text is already pushed continuously by the app;
# here we also push a captured pprof gzip and an OTLP-profiles protobuf), and
# leave a marker so re-runs are visibly idempotent.

# pprof (gzipped protobuf) → /profiles/v1/pprof
if [ -f "$HERE/../fixtures/profile.pb.gz" ]; then
  curl -s -X POST "$API/profiles/v1/pprof?service=demo-backend&type=cpu" \
    -H 'content-type: application/octet-stream' \
    --data-binary "@$HERE/../fixtures/profile.pb.gz" >/dev/null 2>&1 \
    && log "pushed captured pprof profile (genuinely exercises /profiles/v1/pprof)"
fi

# OTLP profiles protobuf → /otlp/v1development/profiles
if [ -f "$HERE/../fixtures/otlp-profiles.pb" ]; then
  curl -s -X POST "$API/otlp/v1development/profiles" \
    -H 'content-type: application/x-protobuf' \
    --data-binary "@$HERE/../fixtures/otlp-profiles.pb" >/dev/null 2>&1 \
    && log "pushed captured OTLP profiles protobuf (exercises /otlp/v1development/profiles)"
fi

# a folded text profile for completeness (independent of the app pusher)
printf 'main;handler;db_query 12\nmain;handler;render 7\nmain;gc 3\n' | \
  curl -s -X POST "$API/profiles/v1/folded?service=provision-smoke&type=cpu" \
  -H 'content-type: text/plain' --data-binary @- >/dev/null 2>&1 \
  && log "pushed a folded-text profile (service=provision-smoke)"

log "finalize complete"
