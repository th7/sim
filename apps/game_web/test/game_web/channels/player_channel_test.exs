defmodule GameWeb.PlayerChannelTest do
  use GameWeb.ChannelCase, async: false
  import GameWeb.ChunkCleanup, only: [reset_chunks_and_instances: 1]

  alias GameCore.{Chunk, Chunks, Sessions}

  setup :reset_chunks_and_instances

  defp join_as(username, initial_chunk) do
    GameWeb.UserSocket
    |> socket("user_" <> username, %{})
    |> subscribe_and_join(GameWeb.PlayerChannel, "player:" <> username, %{
      "username" => username,
      "initial_chunk" => Tuple.to_list(initial_chunk)
    })
  end

  test "joining player:<username> starts a Session for that Player" do
    {:ok, _reply, _socket} = join_as("alice", {0, 0})
    assert is_pid(Sessions.whereis("alice"))
  end

  test "a `harvest` event routes through Session to the owning Chunk" do
    {:ok, _reply, socket} = join_as("alice", {0, 0})

    # Player spawns at chunk center (8000, 8000) — on top of the seeded tree.
    ref = push(socket, "harvest", %{"x" => 8000, "y" => 8000})
    assert_reply ref, :ok
  end

  test "a `move` event sets the Player's intent in the current Chunk" do
    {:ok, _reply, socket} = join_as("alice", {0, 0})
    chunk = Chunks.whereis(:overworld, {0, 0})

    push(socket, "move", %{"dx" => 1.0, "dy" => 0.0})
    _ = :sys.get_state(chunk)

    send(chunk, :tick)
    _ = :sys.get_state(chunk)

    %{players: %{"alice" => %{x: x}}} = Chunk.snapshot(chunk)
    assert x > 8000
  end

  test "the client receives `self` pushes via the per-player PubSub topic" do
    {:ok, _reply, _socket} = join_as("alice", {0, 0})

    Phoenix.PubSub.broadcast(
      GameCore.PubSub,
      "self:alice",
      {:self, %{inventory: %{wood: 3}}}
    )

    assert_push "self", %{inventory: %{"wood" => 3}}
  end

  test "channel terminate stops the Session" do
    {:ok, _reply, socket} = join_as("alice", {0, 0})
    spid = Sessions.whereis("alice")
    assert is_pid(spid)

    channel_pid = socket.channel_pid
    Process.unlink(channel_pid)
    ref = Process.monitor(spid)
    leave(socket)

    assert_receive {:DOWN, ^ref, :process, ^spid, _}
  end
end
