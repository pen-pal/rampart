#!/bin/sh
# Wait for provision to mint the Alertmanager ingest token, render the config
# from the template with the real URL, then exec Alertmanager.
set -eu
echo "[alertmanager] waiting for the ingest URL from provision…"
i=0
while [ ! -f /shared/ingest.env ]; do
  i=$((i + 1)); [ "$i" -gt 120 ] && echo "[alertmanager] no ingest.env — exiting" && exit 1
  sleep 2
done
# shellcheck disable=SC1091
. /shared/ingest.env
URL="${ALERTMANAGER_INGEST_URL:-}"
[ -z "$URL" ] && echo "[alertmanager] empty ingest URL — exiting" && exit 1

# Render the config (sed-escape & and / in the URL).
ESC="$(printf '%s' "$URL" | sed -e 's/[&/\]/\\&/g')"
sed "s#__ALERTMANAGER_INGEST_URL__#${ESC}#" /etc/alertmanager/alertmanager.tmpl.yml > /tmp/alertmanager.yml
echo "[alertmanager] posting alerts to $URL"
exec /bin/alertmanager --config.file=/tmp/alertmanager.yml --storage.path=/alertmanager
