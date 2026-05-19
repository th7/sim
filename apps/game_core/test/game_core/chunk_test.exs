defmodule GameCore.ChunkTest do
  use ExUnit.Case, async: true

  alias GameCore.Chunk

  test "new chunk has an empty snapshot" do
    chunk = start_supervised!({Chunk, coord: {0, 0}})
    assert Chunk.snapshot(chunk) == %{players: %{}}
  end

  test "a joined player with no saved state appears at the chunk's center" do
    chunk = start_supervised!({Chunk, coord: {0, 0}})
    :ok = Chunk.join(chunk, "alice")

    # chunk (0,0) spans [0, 16) — center is (8, 8).
    assert %{players: %{"alice" => %{x: 8.0, y: 8.0}}} = Chunk.snapshot(chunk)
  end

  test "leaving removes the player from the snapshot" do
    chunk = start_supervised!({Chunk, coord: {0, 0}})
    :ok = Chunk.join(chunk, "alice")
    :ok = Chunk.join(chunk, "bob")
    :ok = Chunk.leave(chunk, "alice")

    snapshot = Chunk.snapshot(chunk)
    refute Map.has_key?(snapshot.players, "alice")
    assert Map.has_key?(snapshot.players, "bob")
  end

  test "nonzero intent moves the player over ticks" do
    chunk =
      start_supervised!({Chunk, coord: {0, 0}, auto_tick: false, speed: 4.0, tick_ms: 50})

    :ok = Chunk.join(chunk, "alice")
    %{players: %{"alice" => %{x: x0, y: y0}}} = Chunk.snapshot(chunk)

    :ok = Chunk.set_intent(chunk, "alice", {1.0, 0.0})
    send(chunk, :tick)
    send(chunk, :tick)
    _ = :sys.get_state(chunk)

    %{players: %{"alice" => %{x: x1, y: y1}}} = Chunk.snapshot(chunk)
    assert_in_delta x1 - x0, 0.4, 1.0e-9
    assert_in_delta y1 - y0, 0.0, 1.0e-9
  end

  test "zero intent leaves position unchanged across ticks" do
    chunk =
      start_supervised!({Chunk, coord: {0, 0}, auto_tick: false, speed: 4.0, tick_ms: 50})

    :ok = Chunk.join(chunk, "alice")
    :ok = Chunk.set_intent(chunk, "alice", {1.0, 0.0})
    send(chunk, :tick)
    :ok = Chunk.set_intent(chunk, "alice", {0.0, 0.0})

    %{players: %{"alice" => before_pos}} = Chunk.snapshot(chunk)
    send(chunk, :tick)
    send(chunk, :tick)
    _ = :sys.get_state(chunk)

    assert Chunk.snapshot(chunk).players["alice"] == before_pos
  end

  test "subscribers receive a snapshot every other tick" do
    chunk = start_supervised!({Chunk, coord: {0, 0}, auto_tick: false})
    :ok = Chunk.subscribe(chunk, self())
    :ok = Chunk.join(chunk, "alice")

    send(chunk, :tick)
    send(chunk, :tick)
    send(chunk, :tick)
    send(chunk, :tick)
    _ = :sys.get_state(chunk)

    assert_received {:snapshot, %{players: %{"alice" => _}}}
    assert_received {:snapshot, %{players: %{"alice" => _}}}
    refute_received {:snapshot, _}
  end
end
