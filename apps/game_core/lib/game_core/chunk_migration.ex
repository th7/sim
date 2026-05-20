defmodule GameCore.ChunkMigration do
  @moduledoc """
  Hands an entity off from a source Chunk to a destination Chunk —
  implementation of Boundary crossing in `CONTEXT.md`.

  The source's tick detects that a Position has crossed a chunk boundary
  and calls `cross/5` with the entity's components. This module ensures
  the destination Chunk is hot, performs the synchronous handoff, and
  notifies the Player's Session so its Warm set pans to follow. The
  source then removes the entity from its own world.
  """

  alias GameCore.{Chunk, Chunks, Session, Sessions}

  @doc """
  Hand `eid` off from `from_coord` to `to_coord` with the given
  components. Starts the destination Chunk if cold (under `repo`).
  Notifies the entity's Session if one exists. Returns `:ok`.
  """
  @spec cross(
          GameCore.World.eid(),
          from :: Chunk.coord(),
          to :: Chunk.coord(),
          components :: %{module() => any()},
          repo :: module()
        ) :: :ok
  def cross(eid, _from_coord, to_coord, components, repo) do
    {:ok, dest} = Chunks.ensure_started(to_coord, repo)
    :ok = Chunk.migrate_in(dest, eid, components)

    case Sessions.whereis(eid) do
      spid when is_pid(spid) -> Session.relocate(spid, to_coord)
      _ -> :ok
    end

    :ok
  end
end
