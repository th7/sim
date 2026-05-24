defmodule GameCore.ChunkSelfEventsTest do
  @moduledoc """
  Stage 4 wire protocol slice: after Inventory mutations, the Chunk
  publishes a `{:self, %{inventory: ...}}` message to the per-owner
  PubSub topic `"self:<username>"`. Only the owner channel subscribes
  to that topic, so observers never see Inventory state.
  """
  use GameCore.ChunkCase, async: false

  alias GameCore.Chunk

  test "harvest publishes the updated inventory to self:<username>" do
    :ok = Phoenix.PubSub.subscribe(GameCore.PubSub, "self:alice")

    chunk =
      start_supervised!({Chunk, coord: {0, 0}, auto_tick: false, auto_flush: false})

    :ok = Chunk.join(chunk, "alice")

    %{resource_nodes: nodes} = Chunk.snapshot(chunk)
    [{_id, %{x: tx, y: ty}} | _] = Map.to_list(nodes)
    :ok = Chunk.harvest(chunk, "alice", {tx, ty})

    assert_received {:self, %{inventory: %{wood: 1}}}
  end

  test "build publishes the decremented inventory to self:<username>" do
    :ok = Phoenix.PubSub.subscribe(GameCore.PubSub, "self:bob")

    chunk =
      start_supervised!({Chunk, coord: {0, 0}, auto_tick: false, auto_flush: false})

    :ok = Chunk.join(chunk, "bob")
    :ok = Chunk.set_inventory(chunk, "bob", %{wood: 5})
    # Drain the set_inventory self event so we only assert on the build one.
    assert_received {:self, %{inventory: %{wood: 5}}}

    # (12_000, 8_000) sits clear of bob's spawn (8_000, 8_000) and of every
    # Worldgen tree footprint in chunk (0,0).
    :ok = Chunk.build(chunk, "bob", :wall, {12_000, 8_000})

    assert_received {:self, %{inventory: %{wood: 0}}}
  end
end
