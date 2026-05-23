defmodule GameCore.ChunkMigration do
  @moduledoc """
  Hands an entity off from a source Chunk to a destination Chunk in the
  *same realm* — implementation of Boundary crossing in `CONTEXT.md`.
  Cross-realm transitions (Overworld ⇄ Instance) bypass this module and
  go through `GameCore.Session.enter_instance/3` /
  `Session.exit_instance/1` directly, because they need Position override
  and realm-state swaps that don't fit the Boundary-crossing shape.

  The source's tick detects that a Position has crossed a chunk boundary
  and calls `cross/6`. This module ensures the destination Chunk is hot,
  performs the synchronous handoff, and notifies the Player's Session so
  its Warm set pans to follow. The source then removes the entity from
  its own world.
  """

  alias GameCore.{Chunk, Chunks, Session, Sessions}

  @doc """
  Hand `eid` off from `from_coord` to `to_coord` within `realm` with the
  given components. Starts the destination Chunk if cold. Notifies the
  entity's Session if one exists. Returns `:ok`.
  """
  @spec cross(
          Chunks.realm(),
          GameCore.World.eid(),
          from :: Chunk.coord(),
          to :: Chunk.coord(),
          components :: %{module() => any()}
        ) :: :ok
  def cross(realm, eid, _from_coord, to_coord, components) do
    {:ok, dest} = Chunks.ensure_started(realm, to_coord)
    :ok = Chunk.migrate_in(dest, eid, components)

    case Sessions.whereis(eid) do
      spid when is_pid(spid) -> Session.relocate(spid, to_coord)
      _ -> :ok
    end

    :ok
  end
end
