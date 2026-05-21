defmodule GameCore.ChunkDevStatusTest do
  use ExUnit.Case, async: true

  alias GameCore.Chunk

  test "reports :hot with the count of interested pids" do
    chunk = start_supervised!({Chunk, coord: {0, 0}, auto_tick: false, auto_flush: false})

    :ok = Chunk.express_interest(chunk, self())

    status = Chunk.dev_status(chunk)
    assert status.lifecycle == :hot
    assert status.interest_count == 1
  end

  test "reports :idle_armed with a non-nil idle_ms_remaining after the last interest is released" do
    chunk =
      start_supervised!(
        {Chunk, coord: {0, 0}, auto_tick: false, auto_flush: false, idle_timeout_ms: 5_000}
      )

    :ok = Chunk.express_interest(chunk, self())
    :ok = Chunk.release_interest(chunk, self())

    status = Chunk.dev_status(chunk)
    assert status.lifecycle == :idle_armed
    assert is_integer(status.idle_ms_remaining)
    assert status.idle_ms_remaining > 0
    assert status.idle_ms_remaining <= 5_000
    assert status.interest_count == 0
  end

  test "entity_count counts every Positioned entity (players + Worldgen resource nodes)" do
    chunk = start_supervised!({Chunk, coord: {0, 0}, auto_tick: false, auto_flush: false})

    base = Chunk.dev_status(chunk).entity_count

    :ok = Chunk.join(chunk, "alice")
    :ok = Chunk.join(chunk, "bob")

    assert Chunk.dev_status(chunk).entity_count == base + 2

    :ok = Chunk.leave(chunk, "alice")

    assert Chunk.dev_status(chunk).entity_count == base + 1
  end
end
