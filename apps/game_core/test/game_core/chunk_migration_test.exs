defmodule GameCore.ChunkMigrationTest do
  use ExUnit.Case, async: false

  alias GameCore.Chunk
  alias GameCore.Chunks

  setup do
    source =
      start_supervised!(
        {Chunk,
         coord: {0, 0},
         name: Chunks.via(:overworld,{0, 0}),
         auto_tick: false,
         auto_flush: false,
         tick_ms: 50,
         speed: 4_000.0},
        id: :src_chunk
      )

    dest =
      start_supervised!(
        {Chunk,
         coord: {1, 0},
         name: Chunks.via(:overworld,{1, 0}),
         auto_tick: false,
         auto_flush: false,
         tick_ms: 50,
         speed: 4_000.0},
        id: :dst_chunk
      )

    %{source: source, dest: dest}
  end

  test "entity crossing east boundary migrates to neighbor", %{source: src, dest: dst} do
    :ok = Chunk.join(src, "alice")
    :ok = Chunk.set_intent(src, "alice", {1.0, 0.0})

    # Walk from chunk center (8000, 8000) past x=16000; that's 8000 sub-units
    # at speed 4000 sub-units/sec = 2.0s = 40 ticks of 50ms. Do 41 to be safe.
    Enum.each(1..41, fn _ ->
      send(src, :tick)
      _ = :sys.get_state(src)
    end)

    refute Map.has_key?(Chunk.snapshot(src).players, "alice")
    assert Map.has_key?(Chunk.snapshot(dst).players, "alice")
  end

  test "migrated entity keeps its velocity (continues moving)", %{source: src, dest: dst} do
    :ok = Chunk.join(src, "alice")
    :ok = Chunk.set_intent(src, "alice", {1.0, 0.0})

    Enum.each(1..41, fn _ ->
      send(src, :tick)
      _ = :sys.get_state(src)
    end)

    %{players: %{"alice" => %{x: x_after_migrate}}} = Chunk.snapshot(dst)

    # Drive the dest a few ticks; alice should keep moving east.
    Enum.each(1..5, fn _ ->
      send(dst, :tick)
      _ = :sys.get_state(dst)
    end)

    %{players: %{"alice" => %{x: x_later}}} = Chunk.snapshot(dst)
    assert x_later > x_after_migrate
  end

  test "set_intent on a chunk that no longer owns the entity is a no-op",
       %{source: src, dest: dst} do
    :ok = Chunk.join(src, "alice")
    :ok = Chunk.set_intent(src, "alice", {1.0, 0.0})

    Enum.each(1..41, fn _ ->
      send(src, :tick)
      _ = :sys.get_state(src)
    end)

    # alice is now in dst. Hitting src.set_intent shouldn't resurrect her there.
    :ok = Chunk.set_intent(src, "alice", {0.0, 0.0})
    refute Map.has_key?(Chunk.snapshot(src).players, "alice")
    assert Map.has_key?(Chunk.snapshot(dst).players, "alice")
  end

  test "destination broadcasts a snapshot immediately on migrate_in", %{source: src} do
    :ok = Phoenix.PubSub.subscribe(GameCore.PubSub, "chunk:1:0")
    :ok = Chunk.join(src, "alice")
    :ok = Chunk.set_intent(src, "alice", {1.0, 0.0})

    Enum.each(1..41, fn _ ->
      send(src, :tick)
      _ = :sys.get_state(src)
    end)

    # We want to see at least one snapshot containing alice delivered to
    # dst's topic as a result of the migration (NOT as a result of a
    # separate dst tick — we never ticked dst).
    assert_received {:snapshot, %{players: %{"alice" => _}}}
  end
end
