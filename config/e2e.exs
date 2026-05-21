import Config

# E2e env: a dedicated BEAM for Playwright runs. Lives alongside :dev so
# both can run simultaneously. Port is supplied by runtime.exs via PORT
# (Playwright's webServer sets PORT=4001); DB is sim_e2e.

config :game_core, chunk_repo: GamePersistence.ChunkRepo

config :game_persistence, GamePersistence.Repo,
  database: "sim_e2e",
  pool_size: 10

config :game_web, GameWeb.Endpoint,
  http: [ip: {0, 0, 0, 0}],
  check_origin: false,
  code_reloader: false,
  debug_errors: false,
  secret_key_base: "zQ8wbX1ZpKnYJ4t6F2sH3aLgVqRwM5cE+oNuT0iC9rD7yA8hBfXEK1jUZvOWmPxk",
  watchers: []

config :logger, level: :info

config :phoenix, :plug_init_mode, :runtime
