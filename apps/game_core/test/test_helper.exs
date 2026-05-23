ExUnit.start(exclude: [:slow])

# Chunk tests start a per-test `GamePersistence.Datastore` (see ChunkCase)
# under an Ecto SQL sandbox checkout. Ensure the persistence app and its
# deps are up so Repo + DBConnection protocols are loaded.
{:ok, _} = Application.ensure_all_started(:game_persistence)

Ecto.Adapters.SQL.Sandbox.mode(GamePersistence.Repo, :manual)
