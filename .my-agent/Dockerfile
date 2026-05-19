# Phase 0 toolchain: Elixir/OTP (Phoenix), inotify-tools (Phoenix file
# watcher), and Postgres (server + client). The Postgres data directory
# lives under the bind-mounted /workspace so it survives container rebuilds.
# Node is already provided by the base image.

USER root

# Install postgresql-common first so we can disable Debian's auto-cluster
# creation before the postgresql package's postinst runs — otherwise an
# unused cluster lands at /var/lib/postgresql/17/main, since we run our own
# cluster at $PGDATA (see pg-start).
RUN apt-get update \
    && apt-get install -y --no-install-recommends postgresql-common \
    && sed -ri 's/^#?\s*create_main_cluster\s*=.*$/create_main_cluster = false/' \
         /etc/postgresql-common/createcluster.conf \
    && apt-get install -y --no-install-recommends \
      build-essential \
      ca-certificates \
      curl \
      git \
      inotify-tools \
      locales \
      elixir \
      postgresql \
      postgresql-client \
    && rm -rf /var/lib/apt/lists/*

# Erlang prefers UTF-8.
RUN sed -i 's/^# *en_US.UTF-8/en_US.UTF-8/' /etc/locale.gen && locale-gen
ENV LANG=en_US.UTF-8 \
    LC_ALL=en_US.UTF-8

# Symlink the versioned Postgres binaries onto PATH so we don't have to track
# the Debian-shipped major version here. Postgres refuses to run as root, so
# pg-start (below) runs the cluster as the agent user with PGDATA under the
# bind-mounted /workspace.
RUN PG_BIN="$(ls -d /usr/lib/postgresql/*/bin | sort -V | tail -1)" \
    && for f in postgres initdb pg_ctl pg_isready psql createdb dropdb; do \
         ln -sf "$PG_BIN/$f" "/usr/local/bin/$f"; \
       done

# Debian Trixie's erlang-public-key strips the .hrl include headers, which
# breaks Phoenix compile (lib/mix/tasks/phx.gen.cert.ex uses Record.extract
# on public_key/include/OTP-PUB-KEY.hrl). The upstream OTP source doesn't
# ship the file directly — it's generated at OTP build time from the ASN.1
# specs in lib/public_key/asn1. We pre-generated it once with the system
# asn1 compiler (`erlc -bber +der +noobj OTP-PUB-KEY.set.asn` against the
# OTP maint-27 asn1 sources) and checked it into the repo so the build is
# offline-reproducible.
COPY .my-agent/OTP-PUB-KEY.hrl /tmp/OTP-PUB-KEY.hrl
RUN PK_DIR="$(ls -d /usr/lib/erlang/lib/public_key-*)" \
    && mkdir -p "$PK_DIR/include" \
    && mv /tmp/OTP-PUB-KEY.hrl "$PK_DIR/include/OTP-PUB-KEY.hrl"

ENV PGDATA=/workspace/.pgdata \
    PGHOST=127.0.0.1 \
    PGPORT=5432 \
    PGUSER=postgres

COPY .my-agent/scripts/pg-start.sh /usr/local/bin/pg-start
RUN chmod +x /usr/local/bin/pg-start

# Docker's `USER` directive sets uid/gid but not $HOME — without this, mix
# would default to /root/.mix at build time. And my-agent mounts a persistent
# docker volume over /home/agent at runtime, so anything the build wrote
# under /home/agent/.mix would be masked at startup anyway. Park Mix's state
# at /opt/mix (image-internal, not under the home volume) so the archives
# survive the mount.
ENV HOME=/home/agent \
    MIX_HOME=/opt/mix
RUN mkdir -p /opt/mix && chown -R agent:agent /opt/mix

USER agent

# Hex + Rebar for the agent user so `mix deps.get` works without prompting.
# Phoenix installer (`mix phx.new`) is the entry point for the Phase 0 scaffold.
RUN mix local.hex --force \
    && mix local.rebar --force \
    && mix archive.install --force hex phx_new

WORKDIR /workspace
