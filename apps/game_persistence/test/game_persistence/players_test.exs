defmodule GamePersistence.PlayersTest do
  use GamePersistence.DataCase, async: true

  alias GamePersistence.Players

  test "get_or_create spawns a player at chunk-(0,0) centre on first sight" do
    pos = Players.get_or_create("alice")

    assert pos == %{
             username: "alice",
             chunk_x: 0,
             chunk_y: 0,
             x: 8_000,
             y: 8_000,
             inventory: %{}
           }
  end

  test "get_or_create is idempotent and preserves a saved position" do
    Players.get_or_create("alice")
    first = Players.get_or_create("alice")
    second = Players.get_or_create("alice")
    assert first == second
  end

  test "upsert_position persists changes that get_or_create reads back" do
    Players.get_or_create("alice")
    :ok = Players.upsert_position("alice", {1, -2}, 7_500, 3_250)

    assert %{chunk_x: 1, chunk_y: -2, x: 7_500, y: 3_250} = Players.get_or_create("alice")
  end

  test "upsert_position is a no-op for an unknown user" do
    assert :ok = Players.upsert_position("ghost", {0, 0}, 1_000, 2_000)
  end
end
