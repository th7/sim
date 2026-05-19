defmodule GamePersistence.ChunkRepoTest do
  use GamePersistence.DataCase, async: true

  alias GamePersistence.ChunkRepo

  test "fetch_player returns saved coord+position; flush_players round-trips" do
    initial = ChunkRepo.fetch_player("alice")
    assert %{username: "alice", chunk_x: 0, chunk_y: 0, x: +0.0, y: +0.0} = initial

    :ok = ChunkRepo.flush_players({2, -1}, [%{username: "alice", x: 4.0, y: 5.0}])

    assert %{chunk_x: 2, chunk_y: -1, x: 4.0, y: 5.0} = ChunkRepo.fetch_player("alice")
  end

  test "flush_players is a no-op for unknown users" do
    assert :ok = ChunkRepo.flush_players({0, 0}, [%{username: "ghost", x: 1.0, y: 2.0}])
  end
end
