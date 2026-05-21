defmodule GamePersistence.ChunkRepoTest do
  use GamePersistence.DataCase, async: true

  alias GamePersistence.ChunkRepo

  test "fetch_player returns saved coord+position; flush_players round-trips" do
    initial = ChunkRepo.fetch_player("alice")
    assert %{username: "alice", chunk_x: 0, chunk_y: 0, x: 8_000, y: 8_000} = initial

    :ok = ChunkRepo.flush_players({2, -1}, [%{username: "alice", x: 4_000, y: 5_000}])

    assert %{chunk_x: 2, chunk_y: -1, x: 4_000, y: 5_000} = ChunkRepo.fetch_player("alice")
  end

  test "flush_players is a no-op for unknown users" do
    assert :ok = ChunkRepo.flush_players({0, 0}, [%{username: "ghost", x: 1_000, y: 2_000}])
  end
end
