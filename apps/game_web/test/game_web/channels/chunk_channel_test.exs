defmodule GameWeb.ChunkChannelTest do
  use GameWeb.ChannelCase, async: false

  alias GameCore.Chunk

  setup do
    chunk =
      start_supervised!(
        {Chunk, coord: {0, 0}, auto_tick: false, name: GameCore.Chunks.via({0, 0})}
      )

    %{chunk: chunk}
  end

  defp join_as(username) do
    GameWeb.UserSocket
    |> socket("user_" <> username, %{})
    |> subscribe_and_join(GameWeb.ChunkChannel, "chunk:0:0", %{"username" => username})
  end

  test "joining chunk:0:0 with a username puts that player in the chunk", %{chunk: chunk} do
    {:ok, _reply, _socket} = join_as("alice")
    assert %{players: %{"alice" => _}} = Chunk.snapshot(chunk)
  end

  test "a `move` event updates the player's intent in the chunk", %{chunk: chunk} do
    {:ok, _reply, socket} = join_as("alice")

    push(socket, "move", %{"dx" => 1.0, "dy" => 0.0})
    _ = :sys.get_state(chunk)

    send(chunk, :tick)
    _ = :sys.get_state(chunk)

    %{players: %{"alice" => %{x: x}}} = Chunk.snapshot(chunk)
    assert x > 0.0
  end

  test "the client receives snapshot pushes", %{chunk: chunk} do
    {:ok, _reply, _socket} = join_as("alice")

    send(chunk, :tick)
    send(chunk, :tick)

    assert_push "snapshot", %{players: %{"alice" => _}}
  end

  test "leaving the channel removes the player from the chunk", %{chunk: chunk} do
    {:ok, _reply, socket} = join_as("alice")

    channel_pid = socket.channel_pid
    Process.unlink(channel_pid)
    ref = Process.monitor(channel_pid)
    leave(socket)
    assert_receive {:DOWN, ^ref, :process, ^channel_pid, _}

    refute Map.has_key?(Chunk.snapshot(chunk).players, "alice")
  end
end
