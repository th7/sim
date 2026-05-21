defmodule GameWeb.ChunkChannelPostMigrationTest do
  @moduledoc """
  Regression: clicking a tree after the player crossed a Chunk boundary
  must not crash the home chunk. The owner channel stays bound to the
  home chunk for its lifetime, so interact verbs (`harvest`, `build`,
  `damage`) must route through the Session — which tracks the chunk
  that currently owns the entity — rather than directly to the home
  chunk's coord.
  """
  use GameWeb.ChannelCase, async: false

  alias GameCore.{Chunk, Chunks}

  setup do
    src =
      start_supervised!(
        {Chunk, coord: {0, 0}, name: Chunks.via({0, 0}), auto_tick: false, auto_flush: false},
        id: :src_chunk
      )

    dst =
      start_supervised!(
        {Chunk, coord: {1, 0}, name: Chunks.via({1, 0}), auto_tick: false, auto_flush: false},
        id: :dst_chunk
      )

    %{src: src, dst: dst}
  end

  defp join_owner(username) do
    GameWeb.UserSocket
    |> socket("user_" <> username, %{})
    |> subscribe_and_join(GameWeb.ChunkChannel, "chunk:0:0", %{"username" => username})
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
    {:ok, _reply, socket} = join_owner("alice")
    src_ref = Process.monitor(src)

    push(socket, "move", %{"dx" => 1.0, "dy" => 0.0})
    # Flush the channel so the move intent reaches src BEFORE we tick.
    _ = :sys.get_state(socket.channel_pid)
    tick_until_migrated(src, dst, "alice")

    # Walk Alice into a tree's interact range: trees in (1,0) cluster at
    # chunk-center (24000, 8000) ± 500 sub-units, migration drops her near
    # x=16200, and she moves 200 sub-units/tick east. ~35 ticks lands her
    # at x≈23200 — well within 1u of the west-edge trees.
    Enum.each(1..35, fn _ ->
      send(dst, :tick)
      _ = :sys.get_state(dst)
    end)

    # Stop her so the next-tick migration check doesn't move her over a tree.
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
    assert Process.alive?(src), "home chunk crashed handling post-migration harvest"

    assert Chunk.player_inventory(dst, "alice") == %{wood: 1}
    assert Chunk.player_inventory(src, "alice") == %{}
  end

  test "harvest on a stale src directly still does not crash the chunk",
       %{src: src, dst: dst} do
    {:ok, _reply, socket} = join_owner("alice")

    push(socket, "move", %{"dx" => 1.0, "dy" => 0.0})
    _ = :sys.get_state(socket.channel_pid)
    tick_until_migrated(src, dst, "alice")

    # Even the defensive chunk-side path returns a structured error.
    assert {:error, :no_player} = Chunk.harvest(src, "alice", {7500, 7500})
    assert Process.alive?(src)
  end
end
