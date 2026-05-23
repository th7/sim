defmodule GameCore.ChunkRespawnTest do
  @moduledoc """
  Resource nodes respawn on a timer after being harvested. The Chunk
  schedules a `Process.send_after(self(), {:respawn, eid}, respawn_ms)`
  on harvest; when the message fires it flips the entity from `Depleted`
  back to `Gatherable` and broadcasts a snapshot.

  Respawn behaviour is in-memory; persistence rides the heartbeat
  (`flush_db`) and is covered separately.
  """
  use GameCore.ChunkCase, async: false

  alias GameCore.Chunk

  test "tracer: a harvested tree becomes Gatherable again after respawn_ms" do
    chunk =
      start_supervised!(
        {Chunk, coord: {0, 0}, auto_tick: false, auto_flush: false, respawn_ms: 30}
      )

    :ok = Chunk.join(chunk, "alice")
    %{resource_nodes: nodes} = Chunk.snapshot(chunk)
    [{id, %{x: tx, y: ty, depleted: false}} | _] = Map.to_list(nodes)

    :ok = Chunk.harvest(chunk, "alice", {tx, ty})
    assert Chunk.snapshot(chunk).resource_nodes[id].depleted == true

    # Drain the timer: wait up to 200ms for the {:respawn, _} message to
    # have been processed by the GenServer. We poll via :sys.get_state/1
    # which both flushes the mailbox and is timing-deterministic.
    assert wait_until(200, fn ->
             _ = :sys.get_state(chunk)
             Chunk.snapshot(chunk).resource_nodes[id].depleted == false
           end)
  end

  test "during the respawn window, the node stays depleted and re-harvest fails" do
    chunk =
      start_supervised!(
        {Chunk, coord: {0, 0}, auto_tick: false, auto_flush: false, respawn_ms: 10_000}
      )

    :ok = Chunk.join(chunk, "alice")
    %{resource_nodes: nodes} = Chunk.snapshot(chunk)
    [{id, %{x: tx, y: ty}} | _] = Map.to_list(nodes)

    :ok = Chunk.harvest(chunk, "alice", {tx, ty})

    # Window is 10s wide so we definitely sit inside it for this assertion.
    assert Chunk.snapshot(chunk).resource_nodes[id].depleted == true
    assert {:error, :depleted} = Chunk.harvest(chunk, "alice", {tx, ty})
    # Inventory does not double up on the rejected second harvest.
    assert Chunk.player_inventory(chunk, "alice") == %{wood: 1}
  end

  test "harvest -> respawn -> harvest cycles and yields wood twice" do
    chunk =
      start_supervised!(
        {Chunk, coord: {0, 0}, auto_tick: false, auto_flush: false, respawn_ms: 30}
      )

    :ok = Chunk.join(chunk, "alice")
    %{resource_nodes: nodes} = Chunk.snapshot(chunk)
    [{id, %{x: tx, y: ty}} | _] = Map.to_list(nodes)

    :ok = Chunk.harvest(chunk, "alice", {tx, ty})
    assert Chunk.player_inventory(chunk, "alice") == %{wood: 1}

    assert wait_until(200, fn ->
             _ = :sys.get_state(chunk)
             Chunk.snapshot(chunk).resource_nodes[id].depleted == false
           end)

    :ok = Chunk.harvest(chunk, "alice", {tx, ty})
    assert Chunk.player_inventory(chunk, "alice") == %{wood: 2}
    assert Chunk.snapshot(chunk).resource_nodes[id].depleted == true
  end

  test "respawn broadcasts a snapshot to chunk subscribers without waiting for a tick" do
    :ok = Phoenix.PubSub.subscribe(GameCore.PubSub, "chunk:7:7")

    chunk =
      start_supervised!(
        {Chunk, coord: {7, 7}, auto_tick: false, auto_flush: false, respawn_ms: 30}
      )

    :ok = Chunk.join(chunk, "alice")
    %{resource_nodes: nodes} = Chunk.snapshot(chunk)
    [{id, %{x: tx, y: ty}} | _] = Map.to_list(nodes)

    :ok = Chunk.harvest(chunk, "alice", {tx, ty})

    # Auto-tick is off, so the ONLY snapshot we can receive is the one
    # the respawn handler is supposed to publish out-of-cycle.
    assert_receive {:snapshot, %{resource_nodes: %{^id => %{depleted: false}}}}, 200
  end

  defp wait_until(timeout_ms, fun) when timeout_ms <= 0, do: fun.()

  defp wait_until(timeout_ms, fun) do
    if fun.() do
      true
    else
      Process.sleep(5)
      wait_until(timeout_ms - 5, fun)
    end
  end
end
