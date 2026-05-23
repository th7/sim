defmodule GameCore.SessionBackpressureTest do
  @moduledoc """
  The full verb chain — PlayerChannel → Session → Chunk → Datastore —
  must freeze gracefully (not crash) when the Datastore is in
  backpressure. Tests here exercise the Session layer specifically;
  Chunk and Datastore wrappers have their own backpressure tests.

  Slow by construction: same ~5.5s wait reasoning as the lower layers.
  """
  use GameCore.ChunkCase, async: false

  alias GameCore.Session
  alias GamePersistence.Datastore

  @tag :slow
  test "Session.harvest waits :infinity through Session → Chunk → Datastore" do
    Process.flag(:trap_exit, true)

    stop_supervised!(Datastore)
    start_supervised!({Datastore, n_high: 2, n_low: 1, flush_interval_ms: 0})

    # warm_radius: 0 keeps the Session's warm set to just the center chunk
    # so we don't pay activation cost for 25 chunks per test.
    {:ok, sess} =
      Session.start_link(username: "alice", initial_chunk: {0, 0}, warm_radius: 0)

    # Sync: ensure init's Chunk.join finished before we engage backpressure.
    _ = :sys.get_state(sess)

    :ok = Datastore.upsert_player("seed1", {0, 0}, 1, 2, %{})
    :ok = Datastore.upsert_player("seed2", {0, 0}, 3, 4, %{})
    assert :backpressured = Datastore.mode()

    # The tree at chunk-center is always at {8000, 8000} per Worldgen.
    task = Task.async(fn -> Session.harvest(sess, {8_000, 8_000}) end)

    refute Task.yield(task, 5_500),
           "expected Session.harvest to wait :infinity through the full verb chain"

    :ok = Datastore.flush_now()

    assert {:ok, :ok} = Task.yield(task, 1_000) || Task.shutdown(task)
  end
end
