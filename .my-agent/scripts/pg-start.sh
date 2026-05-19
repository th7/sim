#!/usr/bin/env bash
# Boots the in-container Postgres on demand. Initdb on first run, otherwise
# just start if not already running. Listens on 127.0.0.1 only.
set -euo pipefail

: "${PGDATA:=/workspace/.pgdata}"
: "${PGPORT:=5432}"

if [ ! -s "$PGDATA/PG_VERSION" ]; then
  mkdir -p "$PGDATA"
  initdb -D "$PGDATA" -U postgres -A trust --encoding=UTF8 --locale=en_US.UTF-8 >/dev/null
fi

if pg_ctl -D "$PGDATA" status >/dev/null 2>&1; then
  echo "postgres already running"
else
  pg_ctl -D "$PGDATA" \
    -l "$PGDATA/postgres.log" \
    -o "-p $PGPORT -h 127.0.0.1 -k /tmp" \
    start
fi
