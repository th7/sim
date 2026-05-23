defmodule GameCore.ChunkPostMigrationHarvestTest do
  @moduledoc """
  Repro for: after a player migrates out of the home chunk, clicking any
  tree causes the game to reset (the chunk that the click is routed to
  crashes, the channel dies, supervisor restarts).

  Frontend routes `harvest` to the *home* chunk channel even after the
  player's entity has migrated to a neighbor — so the home Chunk
  receives a `harvest` for a username whose Position has been removed
  from its World.
  """
  use ExUnit.Case, async: false

  alias GameCore.{Chunk, Chunks}

  setup do
    src =
      start_supervised!(
        {Chunk, coord: {0, 0}, name: Chunks.via(:overworld,{0, 0}), auto_tick: false, auto_flush: false},
        id: :src_chunk
      )

    dst =
      start_supervised!(
        {Chunk, coord: {1, 0}, name: Chunks.via(:overworld,{1, 0}), auto_tick: false, auto_flush: false},
        id: :dst_chunk
      )

    %{src: src, dst: dst}
  end

  test "harvest on home chunk after the player has migrated out does not crash the chunk",
       %{src: src, dst: dst} do
    :ok = Chunk.join(src, "alice")
    :ok = Chunk.set_intent(src, "alice", {1.0, 0.0})

    Enum.each(1..41, fn _ ->
      send(src, :tick)
      _ = :sys.get_state(src)
    end)

    refute Map.has_key?(Chunk.snapshot(src).players, "alice")
    assert Map.has_key?(Chunk.snapshot(dst).players, "alice")

    # Pick a tree in the home chunk (its world still has the resource nodes).
    %{resource_nodes: nodes} = Chunk.snapshot(src)
    [{_id, %{x: tx, y: ty}} | _] = Map.to_list(nodes)

    ref = Process.monitor(src)

    result = Chunk.harvest(src, "alice", {tx, ty})

    # The chunk MUST NOT die. If it does, we'll see a DOWN before assert.
    refute_received {:DOWN, ^ref, :process, ^src, _reason}
    assert Process.alive?(src), "home chunk crashed handling a stale harvest"

    # We expect a structured error, not a crash-induced exit.
    assert match?({:error, _}, result), "expected {:error, _}, got #{inspect(result)}"
  end
end
