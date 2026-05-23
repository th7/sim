defmodule GamePersistence.DatastoreTest do
  use GamePersistence.DataCase, async: false

  alias GamePersistence.Datastore
  alias GamePersistence.Schemas.{Player, ResourceNode, Structure}

  setup do
    start_supervised!(Datastore)
    :ok
  end

  test "an upserted Player is visible via fetch_player" do
    :ok = Datastore.upsert_player("alice", {0, 0}, 1234, 5678, %{wood: 3})

    assert %{
             username: "alice",
             chunk_x: 0,
             chunk_y: 0,
             x: 1234,
             y: 5678,
             inventory: %{wood: 3}
           } = Datastore.fetch_player("alice")
  end

  test "a later upsert overwrites the earlier one for the same Player" do
    :ok = Datastore.upsert_player("alice", {0, 0}, 100, 200, %{wood: 1})
    :ok = Datastore.upsert_player("alice", {1, 0}, 500, 600, %{wood: 5})

    assert %{
             chunk_x: 1,
             chunk_y: 0,
             x: 500,
             y: 600,
             inventory: %{wood: 5}
           } = Datastore.fetch_player("alice")
  end

  test "build_structure makes the Structure visible via fetch_structures" do
    :ok = Datastore.upsert_structure({0, 0}, "alice", :wall, 1_000, 2_000, 100)

    assert [%{type: :wall, owner: "alice", x: 1_000, y: 2_000, hp: 100}] =
             Datastore.fetch_structures({0, 0})
  end

  test "delete_structure tombstones a previously-built Structure" do
    :ok = Datastore.upsert_structure({0, 0}, "alice", :wall, 1_000, 2_000, 100)
    :ok = Datastore.delete_structure(1_000, 2_000)

    assert [] = Datastore.fetch_structures({0, 0})
  end

  test "fetch_player reads from the DB when pending is empty" do
    {:ok, _} =
      %{username: "bob", chunk_x: 1, chunk_y: 2, x: 3_000, y: 4_000, inventory: %{"wood" => 7}}
      |> Player.create_changeset()
      |> Repo.insert()

    assert %{
             username: "bob",
             chunk_x: 1,
             chunk_y: 2,
             x: 3_000,
             y: 4_000,
             inventory: %{wood: 7}
           } = Datastore.fetch_player("bob")
  end

  test "pending entries overlay DB rows on fetch_player" do
    {:ok, _} =
      %{username: "carol", chunk_x: 0, chunk_y: 0, x: 100, y: 200, inventory: %{"wood" => 1}}
      |> Player.create_changeset()
      |> Repo.insert()

    :ok = Datastore.upsert_player("carol", {1, 1}, 999, 888, %{wood: 99})

    assert %{chunk_x: 1, x: 999, y: 888, inventory: %{wood: 99}} =
             Datastore.fetch_player("carol")
  end

  test "pending Player upserts land in the DB after flush_now" do
    :ok = Datastore.upsert_player("dave", {2, 3}, 1234, 5678, %{wood: 3})
    :ok = Datastore.flush_now()

    row = Repo.get_by(Player, username: "dave")
    assert row.chunk_x == 2
    assert row.chunk_y == 3
    assert row.x == 1234
    assert row.y == 5678
    assert row.inventory == %{"wood" => 3}
  end

  test "upsert_player + upsert_structure flush in one transaction" do
    :ok = Datastore.upsert_player("alice", {0, 0}, 500, 600, %{wood: 5})
    :ok = Datastore.upsert_structure({0, 0}, "alice", :wall, 1_000, 2_000, 100)
    :ok = Datastore.flush_now()

    player = Repo.get_by(Player, username: "alice")
    assert player.x == 500
    assert player.inventory == %{"wood" => 5}

    [struct] = Repo.all(Structure)
    assert struct.x == 1_000
    assert struct.y == 2_000
    assert struct.type == "wall"
    assert struct.owner_username == "alice"
    assert struct.hp == 100
  end

  test "pending entries are retained when a flush fails" do
    failing_name = :"datastore_failing_#{System.unique_integer([:positive])}"

    start_supervised!(
      Supervisor.child_spec(
        {Datastore, name: failing_name, repo: GamePersistence.FailingRepo},
        id: failing_name
      )
    )

    :ok = GenServer.call(failing_name, {:upsert_player, "alice", {0, 0}, 1, 2, %{wood: 3}})

    assert {:error, :db_unavailable} = Datastore.flush_now(failing_name)

    pending = Datastore.dump_pending(failing_name)
    assert %{x: 1, y: 2, inventory: %{wood: 3}} = pending.player["alice"]
  end

  test "size threshold parks writes and a flush unblocks them" do
    bp = :"datastore_bp_#{System.unique_integer([:positive])}"

    start_supervised!(
      Supervisor.child_spec(
        {Datastore, name: bp, n_high: 3, n_low: 1},
        id: bp
      )
    )

    for i <- 1..3 do
      :ok = GenServer.call(bp, {:upsert_player, "p#{i}", {0, 0}, i, i, %{}})
    end

    assert :backpressured = Datastore.mode(bp)

    parked =
      Task.async(fn ->
        GenServer.call(bp, {:upsert_player, "p4", {0, 0}, 4, 4, %{}}, 5_000)
      end)

    refute Task.yield(parked, 100), "expected backpressure to park the call"

    :ok = Datastore.flush_now(bp)

    assert {:ok, :ok} = Task.yield(parked, 1_000) || Task.shutdown(parked)
    assert :flowing = Datastore.mode(bp)
  end

  test "upsert_depletion makes the depletion visible via fetch_depletions" do
    until = DateTime.add(DateTime.utc_now(), 30, :second) |> DateTime.truncate(:microsecond)

    :ok = Datastore.upsert_depletion(:overworld, {0, 0}, :tree, 5_000, 6_000, until)

    assert [%{type: :tree, x: 5_000, y: 6_000, depleted_until: ^until}] =
             Datastore.fetch_depletions(:overworld, {0, 0})
  end

  test "delete_depletion tombstones a previously-upserted depletion" do
    until = DateTime.add(DateTime.utc_now(), 30, :second) |> DateTime.truncate(:microsecond)

    :ok = Datastore.upsert_depletion(:overworld, {0, 0}, :tree, 5_000, 6_000, until)
    :ok = Datastore.delete_depletion(:overworld, {0, 0}, :tree, 5_000, 6_000)

    assert [] = Datastore.fetch_depletions(:overworld, {0, 0})
  end

  test "fetch_depletions reads from the DB when pending is empty" do
    until = DateTime.add(DateTime.utc_now(), 60, :second) |> DateTime.truncate(:microsecond)

    {:ok, _} =
      %{chunk_x: 2, chunk_y: 3, type: "tree", x: 7_000, y: 8_000, depleted_until: until}
      |> ResourceNode.changeset()
      |> Repo.insert()

    assert [%{type: :tree, x: 7_000, y: 8_000, depleted_until: ^until}] =
             Datastore.fetch_depletions(:overworld, {2, 3})
  end

  test "pending depletion upserts land in the DB after flush_now" do
    until = DateTime.add(DateTime.utc_now(), 30, :second) |> DateTime.truncate(:microsecond)

    :ok = Datastore.upsert_depletion(:overworld, {4, 5}, :tree, 9_000, 10_000, until)
    :ok = Datastore.flush_now()

    row = Repo.get_by(ResourceNode, chunk_x: 4, chunk_y: 5, x: 9_000, y: 10_000)
    assert row.type == "tree"
    assert row.depleted_until == until
  end

  test "terminate/2 flushes pending writes before the GenServer exits" do
    ds = :"datastore_shutdown_#{System.unique_integer([:positive])}"

    start_supervised!(
      Supervisor.child_spec(
        {Datastore, name: ds},
        id: ds
      )
    )

    :ok = GenServer.call(ds, {:upsert_player, "shutdown_alice", {1, 2}, 999, 888, %{wood: 7}})

    assert is_nil(Repo.get_by(Player, username: "shutdown_alice"))

    :ok = GenServer.stop(ds, :normal)

    player = Repo.get_by(Player, username: "shutdown_alice")
    assert player.x == 999
    assert player.y == 888
    assert player.chunk_x == 1
    assert player.chunk_y == 2
    assert player.inventory == %{"wood" => 7}
  end

  test "age threshold engages backpressure even when size is small" do
    age = :"datastore_age_#{System.unique_integer([:positive])}"

    start_supervised!(
      Supervisor.child_spec(
        {Datastore, name: age, n_high: 1_000, n_low: 200, t_high_ms: 50, t_low_ms: 10},
        id: age
      )
    )

    :ok = GenServer.call(age, {:upsert_player, "alice", {0, 0}, 1, 2, %{}})

    :sys.replace_state(age, fn s ->
      aged = Map.new(s.pending_at, fn {k, t} -> {k, t - 100} end)
      %{s | pending_at: aged}
    end)

    :ok = GenServer.call(age, {:upsert_player, "trigger", {0, 0}, 0, 0, %{}})
    assert :backpressured = Datastore.mode(age)

    parked =
      Task.async(fn ->
        GenServer.call(age, {:upsert_player, "charlie", {0, 0}, 3, 4, %{}}, 5_000)
      end)

    refute Task.yield(parked, 100), "expected age-triggered backpressure to park"

    :ok = Datastore.flush_now(age)

    assert {:ok, :ok} = Task.yield(parked, 1_000) || Task.shutdown(parked)
    assert :flowing = Datastore.mode(age)
  end
end
