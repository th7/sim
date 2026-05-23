defmodule GameCore.SessionInstanceTransitionsTest do
  use GameCore.ChunkCase, async: false

  alias GameCore.{Chunk, Chunks, Session}

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

  test "Inventory survives Instance entry and exit (round-trip)" do
    sess = start_session("alice", {0, 0})

    overworld_chunk = Chunks.whereis(:overworld, {0, 0})
    :ok = Chunk.set_inventory(overworld_chunk, "alice", %{wood: 5})

    :ok = Session.enter_instance(sess, {0, 0}, {4000, 4000})

    {:instance, _id} = Session.current_realm(sess)
    instance_center = Chunks.whereis(Session.current_realm(sess), {1, 1})
    assert Chunk.player_inventory(instance_center, "alice") == %{wood: 5}

    :ok = Session.exit_instance(sess)

    return_chunk = Chunks.whereis(:overworld, {0, 0})
    assert Chunk.player_inventory(return_chunk, "alice") == %{wood: 5}
  end

  test "Multiple Sessions can each occupy their own Instance simultaneously" do
    alice = start_session("alice", {0, 0})
    bob = start_session("bob", {0, 0})

    :ok = Session.enter_instance(alice, {0, 0}, {4000, 4000})
    :ok = Session.enter_instance(bob, {0, 0}, {4000, 4000})

    {:instance, alice_id} = Session.current_realm(alice)
    {:instance, bob_id} = Session.current_realm(bob)
    assert alice_id != bob_id, "each Session must get its own Instance"

    # Each Player is in their own Instance's center chunk; no cross-contamination.
    alice_center = Chunks.whereis({:instance, alice_id}, {1, 1})
    bob_center = Chunks.whereis({:instance, bob_id}, {1, 1})

    assert alice_center != bob_center
    assert Map.has_key?(Chunk.snapshot(alice_center).players, "alice")
    refute Map.has_key?(Chunk.snapshot(alice_center).players, "bob")
    assert Map.has_key?(Chunk.snapshot(bob_center).players, "bob")
    refute Map.has_key?(Chunk.snapshot(bob_center).players, "alice")

    :ok = Session.exit_instance(alice)
    :ok = Session.exit_instance(bob)

    # Both Instances are gone.
    for cx <- 0..2, cy <- 0..2 do
      refute Chunks.whereis({:instance, alice_id}, {cx, cy})
      refute Chunks.whereis({:instance, bob_id}, {cx, cy})
    end
  end

  test "Session.terminate while inside Instance eagerly tears down the Instance" do
    sess = start_session("alice", {0, 0})
    :ok = Session.enter_instance(sess, {0, 0}, {4000, 4000})
    {:instance, id} = Session.current_realm(sess)
    assert is_pid(Chunks.whereis({:instance, id}, {1, 1}))

    # `Session.terminate/2` calls `Instances.terminate(id)` which
    # synchronously waits for the per-Instance supervisor DOWN and the
    # Registry to clear, so by the time we see the Session's own DOWN
    # the Instance chunks are guaranteed gone.
    ref = Process.monitor(sess)
    GenServer.stop(sess)
    assert_receive {:DOWN, ^ref, :process, ^sess, _}, 1_000

    for cx <- 0..2, cy <- 0..2 do
      refute Chunks.whereis({:instance, id}, {cx, cy})
    end
  end
end
