defmodule GameCore.SessionTest do
  use ExUnit.Case, async: false

  alias GameCore.{Chunks, Session}

  setup do
    on_exit(fn ->
      for {_, pid, _, _} <- DynamicSupervisor.which_children(GameCore.ChunkSupervisor) do
        DynamicSupervisor.terminate_child(GameCore.ChunkSupervisor, pid)
      end
    end)

    :ok
  end

  test "on start, the Session activates the warm radius around current chunk" do
    {:ok, sess} =
      Session.start_link(username: "alice", initial_chunk: {0, 0}, warm_radius: 1)

    _ = :sys.get_state(sess)

    for cx <- -1..1, cy <- -1..1 do
      pid = Chunks.whereis({cx, cy})
      assert is_pid(pid), "expected chunk #{cx},#{cy} to be activated"
      assert MapSet.member?(:sys.get_state(pid).interests, sess)
    end

    Process.exit(sess, :shutdown)
  end

  test "on migration, the warm window pans to follow the new chunk" do
    {:ok, sess} =
      Session.start_link(username: "alice", initial_chunk: {0, 0}, warm_radius: 1)

    _ = :sys.get_state(sess)

    Session.on_migrated(sess, {2, 0})
    _ = :sys.get_state(sess)

    # Chunks (-1, *) should no longer be warm; (3, *) should now be.
    for cy <- -1..1 do
      old_pid = Chunks.whereis({-1, cy})
      if is_pid(old_pid), do: refute MapSet.member?(:sys.get_state(old_pid).interests, sess)

      new_pid = Chunks.whereis({3, cy})
      assert is_pid(new_pid)
      assert MapSet.member?(:sys.get_state(new_pid).interests, sess)
    end

    Process.exit(sess, :shutdown)
  end

  test "on terminate, the Session releases all its warm-set interests" do
    {:ok, sess} =
      Session.start_link(username: "alice", initial_chunk: {0, 0}, warm_radius: 1)

    _ = :sys.get_state(sess)

    warmed_pids =
      for cx <- -1..1, cy <- -1..1, do: Chunks.whereis({cx, cy})

    ref = Process.monitor(sess)
    GenServer.stop(sess)
    assert_receive {:DOWN, ^ref, :process, ^sess, _}

    for pid <- warmed_pids, is_pid(pid) do
      refute MapSet.member?(:sys.get_state(pid).interests, sess)
    end
  end
end
