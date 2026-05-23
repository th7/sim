defmodule GameCore.WorldgenPortalsTest do
  use ExUnit.Case, async: false

  alias GameCore.Worldgen

  setup do
    on_exit(fn ->
      for {_, pid, _, _} <- DynamicSupervisor.which_children(GameCore.ChunkSupervisor),
          is_pid(pid) do
        DynamicSupervisor.terminate_child(GameCore.ChunkSupervisor, pid)
      end
    end)

    :ok
  end

  test "portals/1 places one :dungeon Portal at a quarter-offset in chunk {0,0}" do
    # Offset from chunk-center so Players (who spawn at chunk-center on
    # first connect) don't immediately overlap the Portal on join.
    assert Worldgen.portals({0, 0}) == [
             %{type: :dungeon, direction: :into_instance, x: 4000, y: 4000}
           ]
  end

  test "portals/1 returns [] for any other Overworld chunk" do
    assert Worldgen.portals({1, 0}) == []
    assert Worldgen.portals({0, 1}) == []
    assert Worldgen.portals({-3, 5}) == []
    assert Worldgen.portals({999, 999}) == []
  end

  test "a chunk's snapshot includes its worldgen Portals in the `portals` key" do
    {:ok, chunk} =
      DynamicSupervisor.start_child(
        GameCore.ChunkSupervisor,
        {GameCore.Chunk,
         coord: {0, 0},
         name: GameCore.Chunks.via(:overworld, {0, 0}),
         auto_tick: false,
         auto_flush: false}
      )

    on_exit(fn -> Process.exit(chunk, :shutdown) end)

    snap = GameCore.Chunk.snapshot(chunk)
    assert %{portals: portals} = snap
    assert map_size(portals) == 1

    [{wire_id, entry}] = Map.to_list(portals)
    assert wire_id == "portal:dungeon:4000:4000"
    assert entry == %{type: "dungeon", direction: "into_instance", x: 4000, y: 4000}
  end
end
