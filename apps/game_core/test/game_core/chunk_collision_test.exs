defmodule GameCore.ChunkCollisionTest do
  use GameCore.ChunkCase, async: false

  alias GameCore.Chunk
  alias GamePersistence.Datastore

  test "player walking straight at a wall stops flush against the wall's AABB edge" do
    # Seed alice at (4_000, 10_000) — y=10_000 misses the central tree cluster
    # entirely (trees only sit at y ∈ {7_500, 8_000, 8_500}).
    :ok = Datastore.upsert_player("alice", {0, 0}, 4_000, 10_000, %{wood: 5})

    chunk = start_supervised!({Chunk, coord: {0, 0}, auto_tick: false, auto_flush: false})
    :ok = Chunk.join(chunk, "alice")

    # Wall at (12_500, 10_000); 1u-square AABB → x ∈ [12_000, 13_000].
    :ok = Chunk.build(chunk, "alice", :wall, {12_500, 10_000})

    # Full-speed east: 4_000 sub-units/sec × 50 ms tick = 200 sub-units/tick.
    # Distance to flush stop = (12_000 - 300) - 4_000 = 7_700 → ~39 ticks; pad to 50.
    :ok = Chunk.set_intent(chunk, "alice", {1.0, 0.0})
    for _ <- 1..50, do: send(chunk, :tick)

    %{players: %{"alice" => %{x: x, y: y}}} = Chunk.snapshot(chunk)
    assert x == 11_700
    assert y == 10_000
  end

  test "player walking straight at a tree stops where their body just touches the tree's footprint" do
    # Seed alice at (4_000, 8_000) — clear east-line to a Worldgen tree at (7_500, 7_500).
    :ok = Datastore.upsert_player("alice", {0, 0}, 4_000, 8_000, %{})

    chunk = start_supervised!({Chunk, coord: {0, 0}, auto_tick: false, auto_flush: false})
    :ok = Chunk.join(chunk, "alice")

    # Tree circle r=300 at (7_500, 7_500); alice circle r=300 at (?, 8_000).
    # Circles first touch when distance = 600 → (cx - 7_500)² + 500² = 360_000
    # → |cx - 7_500| = √110_000 ≈ 331.66. Clamp away from overlap → 332.
    # Expected flush stop: cx = 7_500 - 332 = 7_168.
    :ok = Chunk.set_intent(chunk, "alice", {1.0, 0.0})
    for _ <- 1..40, do: send(chunk, :tick)

    %{players: %{"alice" => %{x: x, y: y}}} = Chunk.snapshot(chunk)
    assert x == 7_168
    assert y == 8_000
  end

  test "diagonal motion into a wall slides — perpendicular axis zeroed, parallel preserved" do
    # Alice southwest of a wall. Stepping NE: the y-axis would intrude on
    # the wall's south edge, so the y-component is clamped to 0; the x-axis
    # is unconstrained (alice's y-range is just touching the wall's south
    # edge, not overlapping). Result: alice slides east along the wall.
    :ok = Datastore.upsert_player("alice", {0, 0}, 9_200, 9_200, %{wood: 5})

    chunk = start_supervised!({Chunk, coord: {0, 0}, auto_tick: false, auto_flush: false})
    :ok = Chunk.join(chunk, "alice")

    # Wall at (10_000, 10_000) → AABB [9_500, 10_500] × [9_500, 10_500].
    :ok = Chunk.build(chunk, "alice", :wall, {10_000, 10_000})

    :ok = Chunk.set_intent(chunk, "alice", {1.0, 1.0})
    for _ <- 1..3, do: send(chunk, :tick)

    %{players: %{"alice" => %{x: x, y: y}}} = Chunk.snapshot(chunk)
    # 3 ticks of +200 in x; y locked at the southern flush line (cy + r = ay_min).
    assert x == 9_800
    assert y == 9_200
  end

  test "a depleted tree blocks movement identically to a live tree" do
    # Alice southwest of one tree (7_500, 7_500); other trees are too far on
    # this line (y=7_000) to participate. Harvesting transitions Gatherable
    # → Depleted while keeping the Footprint.
    :ok = Datastore.upsert_player("alice", {0, 0}, 7_000, 7_000, %{})

    chunk = start_supervised!({Chunk, coord: {0, 0}, auto_tick: false, auto_flush: false})
    :ok = Chunk.join(chunk, "alice")

    :ok = Chunk.harvest(chunk, "alice", {7_500, 7_500})

    %{resource_nodes: nodes} = Chunk.snapshot(chunk)
    assert Enum.any?(nodes, fn {_id, n} -> n.x == 7_500 and n.y == 7_500 and n.depleted end)

    # Walk east. Same flush stop as in the live-tree test: tree center − √110_000.
    :ok = Chunk.set_intent(chunk, "alice", {1.0, 0.0})
    for _ <- 1..30, do: send(chunk, :tick)

    %{players: %{"alice" => %{x: x, y: y}}} = Chunk.snapshot(chunk)
    assert x == 7_168
    assert y == 7_000
  end

  test "build rejected when the wall AABB would overlap a tree" do
    :ok = Datastore.upsert_player("alice", {0, 0}, 4_000, 4_000, %{wood: 5})

    chunk = start_supervised!({Chunk, coord: {0, 0}, auto_tick: false, auto_flush: false})
    :ok = Chunk.join(chunk, "alice")

    # Wall AABB at (7_500, 7_500) is [7_000, 8_000] × [7_000, 8_000]; the tree
    # at (7_500, 7_500) has a circle Footprint r=300 — fully inside the AABB.
    assert Chunk.build(chunk, "alice", :wall, {7_500, 7_500}) ==
             {:error, :footprint_blocked}
  end

  test "build rejected when the wall AABB would overlap the placing player's body" do
    :ok = Datastore.upsert_player("alice", {0, 0}, 10_000, 10_000, %{wood: 5})

    chunk = start_supervised!({Chunk, coord: {0, 0}, auto_tick: false, auto_flush: false})
    :ok = Chunk.join(chunk, "alice")

    # Placing a wall on alice's own position: the AABB contains her body circle.
    assert Chunk.build(chunk, "alice", :wall, {10_000, 10_000}) ==
             {:error, :footprint_blocked}
  end

  test "build accepted when two walls sit edge-to-edge — AABBs share an edge, no overlap" do
    :ok = Datastore.upsert_player("alice", {0, 0}, 4_000, 10_000, %{wood: 10})

    chunk = start_supervised!({Chunk, coord: {0, 0}, auto_tick: false, auto_flush: false})
    :ok = Chunk.join(chunk, "alice")

    # Two adjacent 1u cells: AABBs share x=10_500 exactly, no overlap (strict <).
    assert :ok = Chunk.build(chunk, "alice", :wall, {10_000, 10_000})
    assert :ok = Chunk.build(chunk, "alice", :wall, {11_000, 10_000})
  end
end
