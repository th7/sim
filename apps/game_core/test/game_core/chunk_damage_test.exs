defmodule GameCore.ChunkDamageTest do
  @moduledoc """
  TDD slice for the damage verb. Tracer bullet asserts the happy path:
  a Wall at full HP takes successive damage calls, dropping HP by the
  fixed 25 each click; on HP ≤ 0 the Structure is deleted via the repo
  and disappears from the snapshot.
  """
  use ExUnit.Case, async: true

  alias GameCore.Chunk

  defmodule StubRepo do
    @moduledoc false
    @behaviour GameCore.ChunkRepo

    def start_link, do: Agent.start_link(fn -> %{deletes: []} end, name: __MODULE__)
    def deletes, do: Agent.get(__MODULE__, & &1.deletes)

    @impl true
    def fetch_player(_), do: nil

    @impl true
    def flush_players(_, _), do: :ok

    @impl true
    def build_structure(_coord, _owner, _type, _x, _y, _inv), do: {:ok, 42}

    @impl true
    def destroy_structure(id) do
      Agent.update(__MODULE__, &%{&1 | deletes: &1.deletes ++ [id]})
      :ok
    end

    @impl true
    def fetch_structures(_coord), do: []
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

  test "damage decrements HP by 25 per click; HP ≤ 0 deletes the Structure" do
    chunk =
      start_supervised!(
        {Chunk, coord: {0, 0}, auto_tick: false, auto_flush: false, repo: StubRepo}
      )

    :ok = Chunk.join(chunk, "alice")
    :ok = Chunk.set_inventory(chunk, "alice", %{wood: 5})

    cell = {8_000, 8_000}
    :ok = Chunk.build(chunk, "alice", :wall, cell)

    [{sid, %{hp: 100}}] = Map.to_list(Chunk.snapshot(chunk).structures)

    :ok = Chunk.damage(chunk, "alice", cell)
    assert Chunk.snapshot(chunk).structures[sid].hp == 75

    :ok = Chunk.damage(chunk, "alice", cell)
    :ok = Chunk.damage(chunk, "alice", cell)
    :ok = Chunk.damage(chunk, "alice", cell)

    # 4 clicks * 25 = 100 → wall removed.
    assert Chunk.snapshot(chunk).structures == %{}
    assert StubRepo.deletes() == [42]
  end

  test "damage out of range: {:error, :too_far} and no state change" do
    chunk =
      start_supervised!(
        {Chunk, coord: {0, 0}, auto_tick: false, auto_flush: false, repo: StubRepo}
      )

    :ok = Chunk.join(chunk, "alice")
    :ok = Chunk.set_inventory(chunk, "alice", %{wood: 5})
    cell = {8_000, 8_000}
    :ok = Chunk.build(chunk, "alice", :wall, cell)

    far_cell = {15_000, 8_000}
    assert {:error, :too_far} = Chunk.damage(chunk, "alice", far_cell)

    [{_sid, wall}] = Map.to_list(Chunk.snapshot(chunk).structures)
    assert wall.hp == 100
  end

  test "damage on a cell with no Structure: {:error, :no_target}" do
    chunk =
      start_supervised!(
        {Chunk, coord: {0, 0}, auto_tick: false, auto_flush: false, repo: StubRepo}
      )

    :ok = Chunk.join(chunk, "alice")
    assert {:error, :no_target} = Chunk.damage(chunk, "alice", {8_000, 8_000})
  end
end
