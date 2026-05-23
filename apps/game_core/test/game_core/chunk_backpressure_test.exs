defmodule GameCore.ChunkBackpressureTest do
  @moduledoc """
  When the Datastore engages backpressure, the Chunk's verb handlers
  freeze rather than crash — that's the design in CONTEXT.md's
  Backpressure entry. Each test forces the Datastore into backpressure,
  issues a verb through the Chunk's public wrapper, asserts the call
  waits past the default `GenServer.call` timeout of 5_000 ms, then
  drains the Datastore and asserts the verb completes.

  Slow by construction (~5.5 s/test): we have to outwait the default
  timeout to prove the wrapper passes `:infinity`.
  """
  use GameCore.ChunkCase, async: false

  alias GameCore.Chunk
  alias GamePersistence.Datastore

  @tag :slow
  test "Chunk.harvest waits :infinity for a backpressured Datastore" do
    # Same trap_exit reasoning as the Datastore-level tracer bullet:
    # Task.async links to the test process.
    Process.flag(:trap_exit, true)

    # ChunkCase started a default Datastore; swap it for one we can
    # force into backpressure with two seed upserts.
    stop_supervised!(Datastore)
    start_supervised!({Datastore, n_high: 2, n_low: 1, flush_interval_ms: 0})

    chunk = start_supervised!({Chunk, coord: {0, 0}, auto_tick: false, auto_flush: false})
    :ok = Chunk.join(chunk, "alice")

    # Engage backpressure before harvesting.
    :ok = Datastore.upsert_player("seed1", {0, 0}, 1, 2, %{})
    :ok = Datastore.upsert_player("seed2", {0, 0}, 3, 4, %{})
    assert :backpressured = Datastore.mode()

    %{resource_nodes: nodes} = Chunk.snapshot(chunk)
    [{_id, %{x: tx, y: ty}} | _] = Map.to_list(nodes)

    task = Task.async(fn -> Chunk.harvest(chunk, "alice", {tx, ty}) end)

    refute Task.yield(task, 5_500),
           "expected Chunk.harvest to wait :infinity through the chunk → datastore chain"

    :ok = Datastore.flush_now()

    assert {:ok, :ok} = Task.yield(task, 1_000) || Task.shutdown(task)
  end

  @tag :slow
  test "Chunk.migrate_in waits :infinity for a backpressured Datastore" do
    Process.flag(:trap_exit, true)

    stop_supervised!(Datastore)
    start_supervised!({Datastore, n_high: 2, n_low: 1, flush_interval_ms: 0})

    dest = start_supervised!({Chunk, coord: {1, 0}, auto_tick: false, auto_flush: false})

    :ok = Datastore.upsert_player("seed1", {0, 0}, 1, 2, %{})
    :ok = Datastore.upsert_player("seed2", {0, 0}, 3, 4, %{})
    assert :backpressured = Datastore.mode()

    # Components shaped like a real Boundary-crossing handoff so the
    # dest's migrate_in handler hits its Datastore.upsert_player branch.
    components = %{
      GameCore.Components.Position => %{x: 16_000, y: 8_000},
      GameCore.Components.Inventory => %{items: %{}},
      GameCore.Components.Renderable => %{},
      GameCore.Components.PlayerControlled => %{},
      GameCore.Components.Velocity => %{vx: 4_000.0, vy: 0.0}
    }

    task = Task.async(fn -> Chunk.migrate_in(dest, "alice", components) end)

    refute Task.yield(task, 5_500),
           "expected Chunk.migrate_in (source-tick path) to wait :infinity past the default 5s"

    :ok = Datastore.flush_now()

    assert {:ok, :ok} = Task.yield(task, 1_000) || Task.shutdown(task)
  end

  # Regression: when the Chunk's verb-path wrappers gained :infinity, the
  # Chunk's lifecycle flush path (terminate → flush_all → safe_emit →
  # Datastore.upsert_player) silently lost its time bound — safe_emit's
  # `catch _, _` no longer fires because an :infinity GenServer.call
  # doesn't EXIT, it blocks. Terminate would then hang until the
  # supervisor force-kills the chunk at its shutdown timeout. This test
  # asserts the lifecycle flush remains best-effort even under
  # backpressure.
  test "Chunk.terminate doesn't hang on a backpressured Datastore" do
    Process.flag(:trap_exit, true)

    stop_supervised!(Datastore)
    start_supervised!({Datastore, n_high: 2, n_low: 1, flush_interval_ms: 0})

    chunk = start_supervised!({Chunk, coord: {0, 0}, auto_tick: false, auto_flush: false})
    :ok = Chunk.join(chunk, "alice")

    :ok = Datastore.upsert_player("seed1", {0, 0}, 1, 2, %{})
    :ok = Datastore.upsert_player("seed2", {0, 0}, 3, 4, %{})
    assert :backpressured = Datastore.mode()

    task = Task.async(fn -> GenServer.stop(chunk, :normal, :infinity) end)

    # Pre-fix: stop hangs until ExUnit force-kills the chunk at the
    # supervisor's shutdown timeout (~5s). Post-fix: terminate's flush
    # fires-and-forgets, returns immediately.
    assert {:ok, :ok} = Task.yield(task, 500),
           "expected Chunk.terminate to complete promptly under backpressure"
  end
end
