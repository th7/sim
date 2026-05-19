defmodule GamePersistence.Repo do
  use Ecto.Repo,
    otp_app: :game_persistence,
    adapter: Ecto.Adapters.Postgres
end
