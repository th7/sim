defmodule GameWeb.PlayerChannelPostMigrationTest do
  @moduledoc """
  Regression: clicking a tree after the Player crossed a Chunk boundary
  must not crash the src chunk. The `PlayerChannel` is persistent — it
  doesn't track the entity's current chunk; the Session does. The verb
  routes through the Session, which knows the entity migrated to dst.
  Tests both the channel-routed happy path and the defensive Chunk-side
  fallback (a direct Chunk.harvest against a stale chunk).
  """
  use GameWeb.ChannelCase, async: false
  import GameWeb.ChunkCleanup, only: [reset_chunks_and_instances: 1]

  alias GameCore.{Chunk, Chunks}

  setup :reset_chunks_and_instances

  setup do
    src =
      start_supervised!(
        {Chunk,
         coord: {0, 0}, name: Chunks.via(:overworld, {0, 0}), auto_tick: false, auto_flush: false},
        id: :src_chunk
      )

    dst =
      start_supervised!(
        {Chunk,
         coord: {1, 0}, name: Chunks.via(:overworld, {1, 0}), auto_tick: false, auto_flush: false},
        id: :dst_chunk
      )

    %{src: src, dst: dst}
  end

  defp join_player(username, initial_chunk) do
    GameWeb.UserSocket
    |> socket("user_" <> username, %{})
    |> subscribe_and_join(GameWeb.PlayerChannel, "player:" <> username, %{
      "username" => username,
      "initial_chunk" => Tuple.to_list(initial_chunk)
    })
  end

  defp tick_until_migrated(src, dst, username, ticks \\ 41) do
    Enum.each(1..ticks, fn _ ->
      send(src, :tick)
      _ = :sys.get_state(src)
    end)

    refute Map.has_key?(Chunk.snapshot(src).players, username)
    assert Map.has_key?(Chunk.snapshot(dst).players, username)
  end

  test "harvest after boundary crossing routes to dest, keeps src alive",
       %{src: src, dst: dst} do
    {:ok, _reply, socket} = join_player("alice", {0, 0})
    src_ref = Process.monitor(src)

    push(socket, "move", %{"dx" => 1.0, "dy" => 0.0})
    _ = :sys.get_state(socket.channel_pid)
    tick_until_migrated(src, dst, "alice")

    Enum.each(1..35, fn _ ->
      send(dst, :tick)
      _ = :sys.get_state(dst)
    end)

    push(socket, "move", %{"dx" => 0.0, "dy" => 0.0})
    _ = :sys.get_state(socket.channel_pid)
    _ = :sys.get_state(dst)

    %{players: %{"alice" => %{x: ax, y: ay}}} = Chunk.snapshot(dst)
    %{resource_nodes: nodes} = Chunk.snapshot(dst)

    {_id, %{x: tx, y: ty}} =
      nodes
      |> Map.to_list()
      |> Enum.min_by(fn {_id, %{x: x, y: y}} ->
        (x - ax) * (x - ax) + (y - ay) * (y - ay)
      end)

    ref = push(socket, "harvest", %{"x" => tx, "y" => ty})
    assert_reply ref, :ok

    refute_received {:DOWN, ^src_ref, :process, ^src, _reason}
    assert Process.alive?(src), "src chunk crashed handling post-migration harvest"

    assert Chunk.player_inventory(dst, "alice") == %{wood: 1}
    assert Chunk.player_inventory(src, "alice") == %{}
  end

  test "harvest on a stale src directly still does not crash the chunk",
       %{src: src, dst: dst} do
    {:ok, _reply, socket} = join_player("alice", {0, 0})

    push(socket, "move", %{"dx" => 1.0, "dy" => 0.0})
    _ = :sys.get_state(socket.channel_pid)
    tick_until_migrated(src, dst, "alice")

    assert {:error, :no_player} = Chunk.harvest(src, "alice", {7500, 7500})
    assert Process.alive?(src)
  end
end
