#!/bin/sh
# Source the secrets the `provision` one-shot captured into the shared volume
# (SENTRY_DSN built from a real error-project public_key + id), then start the
# instrumented app. The file may not exist if you run the app standalone — that
# is fine, the app boots without Sentry.
set -e
if [ -f /shared/demo.env ]; then
  echo "[entrypoint] sourcing /shared/demo.env"
  # shellcheck disable=SC1091
  . /shared/demo.env
  export SENTRY_DSN
fi
exec npm start
