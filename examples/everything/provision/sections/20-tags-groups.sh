# 20-tags-groups.sh — tags (for tag-routing) and monitor folders/groups.

ensure_tag() {  # $1=name $2=color → echoes id
  local existing id
  existing="$(get /v1/tags)"
  id="$(id_by_name "$existing" "$1")"
  [ -n "$id" ] && { echo "$id"; return; }
  post /v1/tags "{\"name\":\"$1\",\"color\":\"$2\"}" | jq -r '.id // empty'
}
ensure_group() {  # $1=name [$2=parent_id] → echoes id
  local existing id body
  existing="$(get /v1/monitor-groups)"
  id="$(id_by_name "$existing" "$1")"
  [ -n "$id" ] && { echo "$id"; return; }
  if [ -n "${2:-}" ]; then body="{\"name\":\"$1\",\"parent_id\":\"$2\"}"; else body="{\"name\":\"$1\"}"; fi
  post /v1/monitor-groups "$body" | jq -r '.id // empty'
}

export TAG_PROD="$(ensure_tag prod '#0d9488')"
export TAG_EDGE="$(ensure_tag edge '#f59e0b')"
export TAG_DATA="$(ensure_tag data '#6366f1')"
export TAG_CRITICAL="$(ensure_tag critical '#ef4444')"
log "tags: prod=$TAG_PROD edge=$TAG_EDGE data=$TAG_DATA critical=$TAG_CRITICAL"

export GRP_CORE="$(ensure_group 'Core services')"
export GRP_EDGE="$(ensure_group 'Edge')"
export GRP_DATASTORES="$(ensure_group 'Datastores' "$GRP_CORE")"   # nested folder
export GRP_NETWORK="$(ensure_group 'Network & protocols')"
export GRP_HEAVY="$(ensure_group 'Heavy (profile)')"
log "groups: core=$GRP_CORE edge=$GRP_EDGE datastores=$GRP_DATASTORES network=$GRP_NETWORK heavy=$GRP_HEAVY"

# persist for later sections (sourced env is in-process, but be explicit)
{
  echo "TAG_PROD=$TAG_PROD TAG_EDGE=$TAG_EDGE TAG_DATA=$TAG_DATA TAG_CRITICAL=$TAG_CRITICAL"
  echo "GRP_CORE=$GRP_CORE GRP_EDGE=$GRP_EDGE GRP_DATASTORES=$GRP_DATASTORES GRP_NETWORK=$GRP_NETWORK GRP_HEAVY=$GRP_HEAVY"
} > "$SHARED/ids.env"
