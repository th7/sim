defmodule GameCore.SessionInstanceTransitionsTest do
  use ExUnit.Case, async: false

  alias GameCore.{Chunk, Chunks, Session}

  setup do
    on_exit(fn ->
      for {_, pid, _, _} <- DynamicSupervisor.which_children(GameCore.ChunkSupervisor),
          is_pid(pid) do
        DynamicSupervisor.terminate_child(GameCore.ChunkSupervisor, pid)
      end

      for {_, pid, _, _} <- DynamicSupervisor.which_children(GameCore.InstancesSupervisor),
          is_pid(pid) do
        DynamicSupervisor.terminate_child(GameCore.InstancesSupervisor, pid)
      end
    end)

    :ok
  end

  defp start_session(username, coord) do
    {:ok, sess} =
      Session.start_link(
        username: username,
        initial_chunk: coord,
        repo: GameCore.ChunkRepo.Null,
        warm_radius: 1
      )

    _ = :sys.get_state(sess)
    sess
  end

  test "enter_instance switches realm, migrates entity to Instance center chunk" do
    sess = start_session("alice", {0, 0})

    assert Session.current_realm(sess) == :overworld
    assert Session.current_chunk(sess) == {0, 0}

    :ok = Session.enter_instance(sess, {0, 0}, {4000, 4000})

    realm = Session.current_realm(sess)
    assert match?({:instance, _id}, realm)
    assert Session.current_chunk(sess) == {1, 1}

    center_pid = Chunks.whereis(realm, {1, 1})
    assert is_pid(center_pid)

    snap = Chunk.snapshot(center_pid)
    assert Map.has_key?(snap.players, "alice"), "alice should be in Instance center chunk"

    # Spawn position is one world unit west of the return-Portal cell (24000, 24000).
    assert %{x: 23_000, y: 24_000} = snap.players["alice"]

    # Source chunk no longer has alice.
    src_pid = Chunks.whereis(:overworld, {0, 0})
    refute Map.has_key?(Chunk.snapshot(src_pid).players, "alice")
  end

  test "exit_instance returns the entity to the Overworld and terminates the Instance" do
    sess = start_session("alice", {0, 0})
    :ok = Session.enter_instance(sess, {0, 0}, {4000, 4000})

    {:instance, id} = Session.current_realm(sess)
    assert is_pid(Chunks.whereis({:instance, id}, {1, 1}))

    :ok = Session.exit_instance(sess)

    assert Session.current_realm(sess) == :overworld
    assert Session.current_chunk(sess) == {0, 0}

    # The Instance is gone: no chunks under the previous realm tag.
    for cx <- 0..2, cy <- 0..2 do
      refute Chunks.whereis({:instance, id}, {cx, cy})
    end

    # Player re-emerges one world unit west of the entry Portal cell (4000, 4000).
    dst_pid = Chunks.whereis(:overworld, {0, 0})
    snap = Chunk.snapshot(dst_pid)
    assert %{x: 3_000, y: 4_000} = snap.players["alice"]
  end

  test "build verb rejected inside an Instance" do
    sess = start_session("alice", {0, 0})
    :ok = Session.enter_instance(sess, {0, 0}, {4000, 4000})

    assert {:error, :no_build_in_instance} = Session.build(sess, :wall, {25_000, 24_000})
  end

  test "Session.terminate while inside Instance eagerly tears down the Instance" do
    sess = start_session("alice", {0, 0})
    :ok = Session.enter_instance(sess, {0, 0}, {4000, 4000})
    {:instance, id} = Session.current_realm(sess)
    assert is_pid(Chunks.whereis({:instance, id}, {1, 1}))

    ref = Process.monitor(sess)
    GenServer.stop(sess)
    assert_receive {:DOWN, ^ref, :process, ^sess, _}, 1_000

    Process.sleep(20)
    for cx <- 0..2, cy <- 0..2 do
      refute Chunks.whereis({:instance, id}, {cx, cy})
    end
  end
end
