import Config

# Tests start chunks themselves under start_supervised; the Session
# warm radius is forced to 0 so it doesn't spawn 25 chunks under the
# global ChunkSupervisor during channel tests.
config :game_core, session_warm_radius: 0

# `PageController` ships the dev shell (references /src/main.ts) regardless
# of whether a prior `mix assets.deploy` left a built index.html in
# priv/static — so the controller test exercises the dev path.
config :game_web, :force_dev_shell, true

config :game_persistence, GamePersistence.Repo,
  database: "sim_test",
  pool: Ecto.Adapters.SQL.Sandbox,
  pool_size: System.schedulers_online() * 2

# We don't run a server during test. If one is required,
# you can enable the server option below.
config :game_web, GameWeb.Endpoint,
  http: [ip: {127, 0, 0, 1}, port: 4002],
  secret_key_base: "2BhsMpWXpa7it1OWwPGzZ+6+qscobsWWzTX6FPi6ImTDZV2R3giYRBJ3r1CksJ/h",
  server: false

# Print only warnings and errors during test
config :logger, level: :warning

# Initialize plugs at runtime for faster test compilation
config :phoenix, :plug_init_mode, :runtime

# Sort query params output of verified routes for robust url comparisons
config :phoenix,
  sort_verified_routes_query_params: true
