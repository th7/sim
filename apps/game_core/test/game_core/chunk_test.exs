defmodule GameCore.ChunkTest do
  use GameCore.ChunkCase, async: false

  alias GameCore.Chunk

  test "new chunk has no players in its snapshot" do
    chunk = start_supervised!({Chunk, coord: {0, 0}})
    snap = Chunk.snapshot(chunk)
    assert snap.players == %{}
  end

  test "a joined player with no saved state appears at the chunk's center" do
    chunk = start_supervised!({Chunk, coord: {0, 0}})
    :ok = Chunk.join(chunk, "alice")

    # chunk (0,0) spans [0, 16000) sub-units — center is (8000, 8000).
    assert %{players: %{"alice" => %{x: 8_000, y: 8_000}}} = Chunk.snapshot(chunk)
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
      start_supervised!({Chunk, coord: {0, 0}, auto_tick: false, speed: 4_000.0, tick_ms: 50})

    :ok = Chunk.join(chunk, "alice")
    %{players: %{"alice" => %{x: x0, y: y0}}} = Chunk.snapshot(chunk)

    :ok = Chunk.set_intent(chunk, "alice", {1.0, 0.0})
    send(chunk, :tick)
    send(chunk, :tick)
    _ = :sys.get_state(chunk)

    %{players: %{"alice" => %{x: x1, y: y1}}} = Chunk.snapshot(chunk)
    # 4000 sub-units/sec * 0.05s/tick * 2 ticks = 400 sub-units.
    assert x1 - x0 == 400
    assert y1 - y0 == 0
  end

  test "zero intent leaves position unchanged across ticks" do
    chunk =
      start_supervised!({Chunk, coord: {0, 0}, auto_tick: false, speed: 4_000.0, tick_ms: 50})

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
    chunk = start_supervised!({Chunk, coord: {77, 77}, auto_tick: false})
    :ok = Phoenix.PubSub.subscribe(GameCore.PubSub, "chunk:77:77")
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
