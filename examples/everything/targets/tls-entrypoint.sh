#!/bin/sh
# Generate two TLS endpoints at startup so the `tls` + `check_cert` monitors see
# REAL certificate state — no fixtures:
#   :8443  self-signed cert that expires in ~10 days → check_cert WARN
#          (cert_expiry_days=14 ⇒ days-left < threshold ⇒ Warn)
#   :9443  cert generated UNDER faketime at 2020 with a 30-day life → already
#          EXPIRED today ⇒ the tls monitor reports Down (expired certificate).
# Served by `openssl s_server` (no extra runtime needed). faketime + openssl
# are installed in the image.
set -e
mkdir -p /certs

# ~10-day cert → check_cert WARN against a 14-day threshold.
openssl req -x509 -newkey rsa:2048 -nodes \
  -keyout /certs/warn.key -out /certs/warn.crt -days 10 \
  -subj "/CN=tls-warn" -addext "subjectAltName=DNS:tls-warn,DNS:localhost" 2>/dev/null

# Already-expired cert: issue it as if it were 2020, 30-day life → long expired.
faketime "2020-01-01 00:00:00" \
  openssl req -x509 -newkey rsa:2048 -nodes \
    -keyout /certs/expired.key -out /certs/expired.crt -days 30 \
    -subj "/CN=tls-expired" -addext "subjectAltName=DNS:tls-expired,DNS:localhost" 2>/dev/null

echo "[tls-target] warn cert (10d) + expired cert (2020) generated"
echo "[tls-target] notAfter warn:    $(openssl x509 -enddate -noout -in /certs/warn.crt)"
echo "[tls-target] notAfter expired: $(openssl x509 -enddate -noout -in /certs/expired.crt)"

# Two TLS listeners. -www serves a tiny HTTP/1.0 page over TLS so an https
# probe gets a 200 too. -quiet keeps logs sane; -naccept makes them loop.
openssl s_server -accept 9443 -cert /certs/expired.crt -key /certs/expired.key -www -quiet &
exec openssl s_server -accept 8443 -cert /certs/warn.crt -key /certs/warn.key -www -quiet
