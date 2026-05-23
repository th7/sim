defmodule GameWeb.ChunkChannelTest do
  use GameWeb.ChannelCase, async: false

  alias GameCore.Chunk

  setup do
    chunk =
      start_supervised!(
        {Chunk, coord: {0, 0}, auto_tick: false, name: GameCore.Chunks.via(:overworld, {0, 0})}
      )

    %{chunk: chunk}
  end

  defp join_observer(username) do
    GameWeb.UserSocket
    |> socket("user_" <> username, %{})
    |> subscribe_and_join(GameWeb.ChunkChannel, "chunk:0:0", %{"username" => username})
  end

  test "joining a chunk topic does not add the Player to the chunk", %{chunk: chunk} do
    {:ok, _reply, _socket} = join_observer("alice")
    refute Map.has_key?(Chunk.snapshot(chunk).players, "alice")
  end

  test "the client receives snapshot pushes", %{chunk: chunk} do
    {:ok, _reply, _socket} = join_observer("alice")

    Chunk.join(chunk, "alice")
    send(chunk, :tick)
    send(chunk, :tick)

    assert_push "snapshot", %{players: %{"alice" => _}}
  end
end
