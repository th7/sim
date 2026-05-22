defmodule GameCore.ChunkHarvestTest do
  @moduledoc """
  TDD slice for the harvest verb. The tracer bullet asserts the happy path
  end-to-end: a Player in range of a Gatherable Resource node calls
  `Chunk.harvest/3` and observes (a) a wood Item appearing in their
  Inventory, and (b) the node flipping to Depleted in the snapshot.

  Worldgen is consulted by `Chunk` at init to seed Resource nodes. The
  test relies on Worldgen placing at least one tree within
  `@interact_range_sq` of chunk-center so the harvest range check passes
  for a freshly-joined Player (who spawns at chunk-center).
  """
  use ExUnit.Case, async: true

  alias GameCore.Chunk

  test "harvest in range: Inventory gains :wood, node flips to Depleted" do
    chunk =
      start_supervised!({Chunk, coord: {0, 0}, auto_tick: false, auto_flush: false})

    :ok = Chunk.join(chunk, "alice")

    %{resource_nodes: nodes_before} = Chunk.snapshot(chunk)
    [{id, %{type: "tree", x: tx, y: ty, depleted: false}} | _] = Map.to_list(nodes_before)

    :ok = Chunk.harvest(chunk, "alice", {tx, ty})

    assert Chunk.player_inventory(chunk, "alice") == %{wood: 1}

    %{resource_nodes: nodes_after} = Chunk.snapshot(chunk)
    assert nodes_after[id].depleted == true
  end

  test "harvest out-of-range returns {:error, :too_far} and does not mutate state" do
    chunk =
      start_supervised!({Chunk, coord: {0, 0}, auto_tick: false, auto_flush: false})

    :ok = Chunk.join(chunk, "alice")

    %{resource_nodes: nodes} = Chunk.snapshot(chunk)
    [{id, %{x: tx, y: ty}} | _] = Map.to_list(nodes)

    # Aim at a position more than 1.0 world unit (1000 sub-units) away from
    # the actual tree — server should see no Gatherable at the queried
    # coords. (Range is checked against the *queried* coords because the
    # server resolves the entity by spatial key.)
    far_x = tx + 5_000
    far_y = ty

    assert {:error, :too_far} = Chunk.harvest(chunk, "alice", {far_x, far_y})

    # Original tree is untouched and inventory still empty.
    assert Chunk.player_inventory(chunk, "alice") == %{}
    %{resource_nodes: still} = Chunk.snapshot(chunk)
    assert still[id].depleted == false
  end

  defmodule InventoryStubRepo do
    @moduledoc false
    @behaviour GameCore.ChunkRepo

    def start_link, do: Agent.start_link(fn -> %{} end, name: __MODULE__)

    @impl true
    def fetch_player(username), do: Agent.get(__MODULE__, & &1[username])

    @impl true
    def flush_players(coord, players) do
      Agent.update(__MODULE__, fn st ->
        Enum.reduce(players, st, fn p, acc ->
          {cx, cy} = coord
          Map.put(acc, p.username, Map.merge(p, %{chunk_x: cx, chunk_y: cy}))
        end)
      end)

      :ok
    end

    @impl true
    def build_structure(_coord, _owner, _type, _x, _y, _inv),
      do: {:error, :not_supported_in_this_stub}

    @impl true
    def destroy_structure(_id), do: :ok

    @impl true
    def fetch_structures(_coord), do: []

    @impl true
    def fetch_depletions(_coord), do: []

    @impl true
    def flush_depletions(_coord, _depleted_now), do: :ok
  end

  test "Inventory survives leave + rejoin via the repo" do
    {:ok, _} = InventoryStubRepo.start_link()

    on_exit(fn ->
      if Process.whereis(InventoryStubRepo), do: Agent.stop(InventoryStubRepo)
    end)

    chunk1 =
      start_supervised!(
        {Chunk, coord: {0, 0}, auto_tick: false, auto_flush: false, repo: InventoryStubRepo},
        id: :chunk1
      )

    :ok = Chunk.join(chunk1, "alice")
    %{resource_nodes: nodes} = Chunk.snapshot(chunk1)
    [{_id, %{x: tx, y: ty}} | _] = Map.to_list(nodes)
    :ok = Chunk.harvest(chunk1, "alice", {tx, ty})
    :ok = Chunk.leave(chunk1, "alice")

    # Fresh chunk instance — must hydrate from the repo.
    :ok = stop_supervised!(:chunk1)

    chunk2 =
      start_supervised!(
        {Chunk, coord: {0, 0}, auto_tick: false, auto_flush: false, repo: InventoryStubRepo},
        id: :chunk2
      )

    :ok = Chunk.join(chunk2, "alice")
    assert Chunk.player_inventory(chunk2, "alice") == %{wood: 1}
  end

  test "harvest of an already-depleted node returns {:error, :depleted}" do
    chunk =
      start_supervised!({Chunk, coord: {0, 0}, auto_tick: false, auto_flush: false})

    :ok = Chunk.join(chunk, "alice")
    %{resource_nodes: nodes} = Chunk.snapshot(chunk)
    [{_id, %{x: tx, y: ty}} | _] = Map.to_list(nodes)

    :ok = Chunk.harvest(chunk, "alice", {tx, ty})
    assert {:error, :depleted} = Chunk.harvest(chunk, "alice", {tx, ty})

    # Inventory does not double up on the second (rejected) call.
    assert Chunk.player_inventory(chunk, "alice") == %{wood: 1}
  end
end
