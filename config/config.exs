# This file is responsible for configuring your umbrella
# and **all applications** and their dependencies with the
# help of the Config module.
#
# Note that all applications in your umbrella share the
# same configuration and dependencies, which is why they
# all use the same configuration file. If you want different
# configurations or dependencies per app, it is best to
# move said applications out of the umbrella.
# This file is responsible for configuring your application
# and its dependencies with the aid of the Config module.
#
# This configuration file is loaded before any dependency and
# is restricted to this project.

# General application configuration
import Config

config :game_web,
  generators: [timestamp_type: :utc_datetime]

# Ecto: game_persistence owns the Repo for the umbrella.
config :game_persistence,
  ecto_repos: [GamePersistence.Repo]

config :game_persistence, GamePersistence.Repo,
  username: "postgres",
  hostname: "127.0.0.1",
  port: 5432

# Configure the endpoint
config :game_web, GameWeb.Endpoint,
  url: [host: "localhost"],
  adapter: Bandit.PhoenixAdapter,
  render_errors: [
    formats: [json: GameWeb.ErrorJSON],
    layout: false
  ],
  pubsub_server: GameWeb.PubSub,
  live_view: [signing_salt: "FBzDB4hD"]

# Configure Elixir's Logger
config :logger, :default_formatter,
  format: "$time $metadata[$level] $message\n",
  metadata: [:request_id]

# Use Jason for JSON parsing in Phoenix
config :phoenix, :json_library, Jason

# Import environment specific config. This must remain at the bottom
# of this file so it overrides the configuration defined above.
import_config "#{config_env()}.exs"

# Sample configuration:
#
#     config :logger, :console,
#       level: :info,
#       format: "$date $time [$level] $metadata$message\n",
#       metadata: [:user_id]
#
