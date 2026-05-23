defmodule GameCore.ChunkSupervisorCascadeTest do
  @moduledoc """
  Regression: when a Player moves one chunk over with the default
  warm_radius=2, an entire column (2 * radius + 1 = 5) of chunks drops out
  of the warm set and arms its idle timer simultaneously. They all
  `:normal`-terminate together when the window elapses.

  If `GameCore.Chunk` were `:permanent`, DynamicSupervisor.ChunkSupervisor
  would try to restart all 5 in lockstep, blow past its default
  `max_restarts: 3 / max_seconds: 5`, and crash itself — taking every live
  Chunk down with it and stranding every Session on a dead `current_chunk`.
  Both tabs in dev then silently lose the ability to move.

  This test pins the Chunk's restart strategy by exercising the cascade.
  """
  use GameCore.ChunkCase, async: false

  alias GameCore.{Chunk, Chunks, Session}

  setup do
    on_exit(fn ->
      try do
        for {_, pid, _, _} <- DynamicSupervisor.which_children(GameCore.SessionSupervisor),
            is_pid(pid) do
          DynamicSupervisor.terminate_child(GameCore.SessionSupervisor, pid)
        end

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

  test "5 chunks idle-deactivating together does not crash ChunkSupervisor" do
    cold_coords = for cy <- -2..2, do: {-2, cy}

    for coord <- cold_coords do
      {:ok, _} =
        DynamicSupervisor.start_child(
          GameCore.ChunkSupervisor,
          {Chunk,
           coord: coord,
           name: Chunks.via(:overworld, coord),
           auto_tick: false,
           auto_flush: false,
           idle_timeout_ms: 50}
        )
    end

    sup_pid = Process.whereis(GameCore.ChunkSupervisor)
    sup_ref = Process.monitor(sup_pid)

    {:ok, sess} = Session.start_link(username: "alice", initial_chunk: {0, 0}, warm_radius: 2)
    _ = :sys.get_state(sess)

    # Move one chunk east: all 5 (-2, *) chunks drop out of the warm set
    # together, arm their 50ms idle timers, and terminate :normal in
    # lockstep ~50ms later.
    Session.relocate(sess, {1, 0})
    _ = :sys.get_state(sess)
    Process.sleep(200)

    refute_received {:DOWN, ^sup_ref, :process, _, _}
    assert Process.alive?(sup_pid), "ChunkSupervisor crashed under a normal idle cascade"

    # And the player can still move on the surviving side of the warm set.
    new_chunk = Chunks.whereis(:overworld, {1, 0})
    assert is_pid(new_chunk)
    Chunk.join(new_chunk, "alice")
    Session.set_intent(sess, {1.0, 0.0})
    send(new_chunk, :tick)
    _ = :sys.get_state(new_chunk)
    assert %{x: x} = Map.get(Chunk.snapshot(new_chunk).players, "alice")
    assert x > 24.0, "alice should have moved east of chunk (1,0)'s lower x bound"

    Process.exit(sess, :shutdown)
  end
end
