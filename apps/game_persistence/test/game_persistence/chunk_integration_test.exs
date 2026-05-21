defmodule GamePersistence.ChunkIntegrationTest do
  @moduledoc """
  End-to-end test of the persistence handshake at the Elixir layer:
  Chunk + GamePersistence.ChunkRepo + Postgres. Simulates a server
  restart by stopping the chunk process and starting a fresh one at the
  same coord; the new chunk must hydrate the Player's last position.
  """
  use GamePersistence.DataCase, async: false

  alias GameCore.Chunk
  alias GamePersistence.ChunkRepo, as: Repo_

  defp start_chunk(coord, id) do
    start_supervised!(
      {Chunk, coord: coord, repo: Repo_, auto_tick: false, auto_flush: false},
      id: id
    )
  end

  test "a player's last position survives a chunk process restart" do
    chunk1 = start_chunk({0, 0}, :chunk1)
    :ok = Chunk.join(chunk1, "alice")
    :ok = Chunk.set_intent(chunk1, "alice", {1.0, 0.0})

    Enum.each(1..4, fn _ ->
      send(chunk1, :tick)
      _ = :sys.get_state(chunk1)
    end)

    :ok = Chunk.leave(chunk1, "alice")

    saved = GamePersistence.Players.get_or_create("alice")
    assert saved.x > 0
    expected = saved.x

    chunk2 = start_chunk({0, 0}, :chunk2)
    :ok = Chunk.join(chunk2, "alice")

    assert %{players: %{"alice" => %{x: x2}}} = Chunk.snapshot(chunk2)
    assert x2 == expected
  end
end
