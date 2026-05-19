defmodule GamePersistence.PlayersTest do
  use GamePersistence.DataCase, async: true

  alias GamePersistence.Players

  test "get_or_create creates a player at the origin on first sight" do
    pos = Players.get_or_create("alice")
    assert pos == %{username: "alice", chunk_x: 0, chunk_y: 0, x: 0.0, y: 0.0}
  end

  test "get_or_create is idempotent and preserves a saved position" do
    Players.get_or_create("alice")
    first = Players.get_or_create("alice")
    second = Players.get_or_create("alice")
    assert first == second
  end

  test "upsert_position persists changes that get_or_create reads back" do
    Players.get_or_create("alice")
    :ok = Players.upsert_position("alice", {1, -2}, 7.5, 3.25)

    assert %{chunk_x: 1, chunk_y: -2, x: 7.5, y: 3.25} = Players.get_or_create("alice")
  end

  test "upsert_position is a no-op for an unknown user" do
    assert :ok = Players.upsert_position("ghost", {0, 0}, 1.0, 2.0)
  end
end
