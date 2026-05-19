defmodule GameCore.SessionLifecycleTest do
  @moduledoc """
  End-to-end test of Phase 6's "world feels infinite" loop: a Session
  warms a 5×5 window around its current chunk; on migration the warm
  window pans, releasing chunks behind and activating chunks ahead on
  demand under the DynamicSupervisor.
  """
  use ExUnit.Case, async: false

  alias GameCore.{Chunk, Chunks, Session}

  setup do
    on_exit(fn ->
      # Tear down every chunk this test left behind. Guard against the
      # ChunkSupervisor not being alive (e.g. when on_exit runs after the
      # app has already started its own shutdown).
      try do
        for {_, pid, _, _} <- DynamicSupervisor.which_children(GameCore.ChunkSupervisor),
            is_pid(pid) do
          DynamicSupervisor.terminate_child(GameCore.ChunkSupervisor, pid)
        end
      catch
        _, _ -> :ok
      end
    end)

    :ok
  end

  test "moving east activates new chunks ahead and releases interest behind" do
    {:ok, sess} =
      Session.start_link(
        username: "alice",
        initial_chunk: {0, 0},
        warm_radius: 2,
        repo: GameCore.ChunkRepo.Null
      )

    _ = :sys.get_state(sess)

    # Pre-move: 5×5 around (0,0) should be alive and hold the Session's
    # interest. Chunk (3,0) is one step east of the warm radius, so cold.
    assert is_pid(Chunks.whereis({-2, 0}))
    assert is_pid(Chunks.whereis({2, 0}))
    refute Chunks.whereis({3, 0})

    # Migrate east; Session pans the warm window.
    Session.on_migrated(sess, {1, 0})
    _ = :sys.get_state(sess)

    assert is_pid(Chunks.whereis({3, 0})), "chunk (3,0) should be activated on demand"
    # Chunk (-2,0) is now outside the warm window; should still be alive
    # but with no Session interest, awaiting idle deactivation.
    minus2 = Chunks.whereis({-2, 0})
    assert is_pid(minus2)
    refute MapSet.member?(:sys.get_state(minus2).interests, sess)

    # Drive a second migration further east.
    Session.on_migrated(sess, {3, 0})
    _ = :sys.get_state(sess)

    # Now even (-1,0) should be released from the Session's warm set.
    minus1 = Chunks.whereis({-1, 0})

    if is_pid(minus1) do
      refute MapSet.member?(:sys.get_state(minus1).interests, sess)
    end

    assert is_pid(Chunks.whereis({5, 0})), "chunk (5,0) should be activated on demand"

    Process.exit(sess, :shutdown)
  end

  test "a chunk with no interested sessions deactivates within the idle window" do
    # Start a chunk with a short idle window, express then release interest,
    # and observe termination.
    {:ok, chunk} =
      DynamicSupervisor.start_child(
        GameCore.ChunkSupervisor,
        {Chunk,
         coord: {77, 77},
         name: Chunks.via({77, 77}),
         auto_tick: false,
         auto_flush: false,
         idle_timeout_ms: 50}
      )

    ref = Process.monitor(chunk)
    :ok = Chunk.express_interest(chunk, self())
    :ok = Chunk.release_interest(chunk, self())

    assert_receive {:DOWN, ^ref, :process, _, _}, 500
    refute Chunks.whereis({77, 77})
  end
end
