ExUnit.start(exclude: [:slow])

Ecto.Adapters.SQL.Sandbox.mode(GamePersistence.Repo, :manual)
