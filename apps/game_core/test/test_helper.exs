ExUnit.start()

# Chunk tests start a per-test `GamePersistence.Datastore` (see ChunkCase)
# under an Ecto SQL sandbox checkout. game_core doesn't depend on
# game_persistence (the dep direction is the reverse, via ChunkRepo,
# pending removal), so its build-path doesn't include game_persistence's
# ebin. Add it manually so `ensure_all_started(:game_persistence)` works.
Code.append_path(
  Path.join([:code.lib_dir(:game_core), "..", "game_persistence", "ebin"])
  |> Path.expand()
)

{:ok, _} = Application.ensure_all_started(:game_persistence)

Ecto.Adapters.SQL.Sandbox.mode(GamePersistence.Repo, :manual)
