defmodule GameCore.ChunkBuildTest do
  @moduledoc """
  TDD slice for the build verb. Tracer bullet asserts the happy path:
  a Player with sufficient materials calls `Chunk.build/4` and observes
  the new Structure appearing in the snapshot, the cost being decremented
  from their Inventory, and the persistence layer being asked to save it.
  """
  use ExUnit.Case, async: true

  alias GameCore.Chunk

  defmodule StubRepo do
    @moduledoc false
    @behaviour GameCore.ChunkRepo

    def start_link, do: Agent.start_link(fn -> %{players: %{}, builds: []} end, name: __MODULE__)
    def builds, do: Agent.get(__MODULE__, & &1.builds)

    @impl true
    def fetch_player(username), do: Agent.get(__MODULE__, & &1.players[username])

    @impl true
    def flush_players(_coord, _players), do: :ok

    @impl true
    def destroy_structure(_id), do: :ok

    @impl true
    def fetch_structures(_coord), do: []

    @impl true
    def build_structure(coord, owner, type, x, y, inventory) do
      next_id =
        Agent.get_and_update(__MODULE__, fn st ->
          id = length(st.builds) + 1

          build_record = %{
            id: id,
            coord: coord,
            owner: owner,
            type: type,
            x: x,
            y: y,
            inventory: inventory
          }

          {id, %{st | builds: st.builds ++ [build_record]}}
        end)

      {:ok, next_id}
    end
  end

  setup do
    {:ok, _} = StubRepo.start_link()
    on_exit(fn ->
      try do
        if Process.whereis(StubRepo), do: Agent.stop(StubRepo)
      catch
        _, _ -> :ok
      end
    end)
    :ok
  end

  test "build :wall with materials in hand: ECS gets Structure, inventory decrements, repo records it" do
    chunk =
      start_supervised!(
        {Chunk, coord: {0, 0}, auto_tick: false, auto_flush: false, repo: StubRepo}
      )

    :ok = Chunk.join(chunk, "alice")
    # Top up alice's wood — bypass the harvest cycle for this slice.
    :ok = Chunk.set_inventory(chunk, "alice", %{wood: 5})

    # Build a wall on the cell at chunk-center (snapped to 1.0u grid).
    {gx, gy} = {8_000, 8_000}
    assert :ok = Chunk.build(chunk, "alice", :wall, {gx, gy})

    # Inventory consumed the cost.
    assert Chunk.player_inventory(chunk, "alice") == %{wood: 0}

    # Snapshot shows the wall.
    %{structures: structures} = Chunk.snapshot(chunk)
    [{_sid, wall}] = Map.to_list(structures)
    assert wall.type == "wall"
    assert wall.x == gx
    assert wall.y == gy
    assert wall.hp == 100
    assert wall.owner == "alice"

    # Repo was asked to persist the wall.
    assert [%{type: :wall, owner: "alice", x: ^gx, y: ^gy, inventory: %{wood: 0}}] =
             StubRepo.builds()
  end

  test "insufficient materials: {:error, :insufficient_materials} and no state change" do
    chunk =
      start_supervised!(
        {Chunk, coord: {0, 0}, auto_tick: false, auto_flush: false, repo: StubRepo}
      )

    :ok = Chunk.join(chunk, "alice")
    :ok = Chunk.set_inventory(chunk, "alice", %{wood: 3})

    assert {:error, :insufficient_materials} =
             Chunk.build(chunk, "alice", :wall, {8_000, 8_000})

    assert Chunk.player_inventory(chunk, "alice") == %{wood: 3}
    assert Chunk.snapshot(chunk).structures == %{}
    assert StubRepo.builds() == []
  end

  test "cell occupied: second build at the same cell returns {:error, :cell_occupied}" do
    chunk =
      start_supervised!(
        {Chunk, coord: {0, 0}, auto_tick: false, auto_flush: false, repo: StubRepo}
      )

    :ok = Chunk.join(chunk, "alice")
    :ok = Chunk.set_inventory(chunk, "alice", %{wood: 10})

    cell = {8_000, 8_000}
    assert :ok = Chunk.build(chunk, "alice", :wall, cell)
    assert {:error, :cell_occupied} = Chunk.build(chunk, "alice", :wall, cell)

    # First build succeeded (cost 5); second was rejected, no additional cost.
    assert Chunk.player_inventory(chunk, "alice") == %{wood: 5}
    assert map_size(Chunk.snapshot(chunk).structures) == 1
  end

  defmodule FailingRepo do
    @moduledoc false
    @behaviour GameCore.ChunkRepo

    @impl true
    def fetch_player(_), do: nil

    @impl true
    def flush_players(_, _), do: :ok

    @impl true
    def build_structure(_, _, _, _, _, _), do: {:error, :db_unavailable}

    @impl true
    def destroy_structure(_id), do: :ok

    @impl true
    def fetch_structures(_coord), do: []
  end

  test "atomicity: a failing build_structure leaves inventory and ECS untouched" do
    chunk =
      start_supervised!(
        {Chunk, coord: {0, 0}, auto_tick: false, auto_flush: false, repo: FailingRepo}
      )

    :ok = Chunk.join(chunk, "alice")
    :ok = Chunk.set_inventory(chunk, "alice", %{wood: 5})

    assert {:error, :db_unavailable} =
             Chunk.build(chunk, "alice", :wall, {8_000, 8_000})

    # No partial state: inventory full, no structure entity.
    assert Chunk.player_inventory(chunk, "alice") == %{wood: 5}
    assert Chunk.snapshot(chunk).structures == %{}
  end
end
