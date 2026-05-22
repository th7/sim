defmodule GamePersistence.ChunkDepletionPersistenceTest do
  @moduledoc """
  End-to-end test of the depletion persistence handshake: harvest mutates
  in-memory, the periodic `:flush_db` tick reconciles the in-memory
  Depleted set with the `resource_nodes` table — INSERT new depletions,
  DELETE rows whose nodes have respawned.
  """
  use GamePersistence.DataCase, async: false

  alias GameCore.Chunk
  alias GamePersistence.ChunkRepo, as: Repo_
  alias GamePersistence.Schemas.ResourceNode

  import Ecto.Query

  defp start_chunk(coord, id, opts) do
    base = [coord: coord, repo: Repo_, auto_tick: false, auto_flush: false]

    start_supervised!(
      {Chunk, Keyword.merge(base, opts)},
      id: id
    )
  end

  defp rows({cx, cy}) do
    Repo.all(from r in ResourceNode, where: r.chunk_x == ^cx and r.chunk_y == ^cy)
  end

  test "flush_db INSERTs a depletion row for a harvested tree" do
    coord = {3, 4}
    chunk = start_chunk(coord, :chunk1, respawn_ms: 60_000)
    :ok = Chunk.join(chunk, "alice")
    %{resource_nodes: nodes} = Chunk.snapshot(chunk)
    [{_id, %{x: tx, y: ty}} | _] = Map.to_list(nodes)

    :ok = Chunk.harvest(chunk, "alice", {tx, ty})
    assert rows(coord) == []

    send(chunk, :flush_db)
    _ = :sys.get_state(chunk)

    assert [%ResourceNode{type: "tree", x: ^tx, y: ^ty, depleted_until: %DateTime{}}] =
             rows(coord)
  end

  test "flush_db DELETEs the row when the node has respawned" do
    coord = {5, 6}
    chunk = start_chunk(coord, :chunk2, respawn_ms: 20)
    :ok = Chunk.join(chunk, "alice")
    %{resource_nodes: nodes} = Chunk.snapshot(chunk)
    [{id, %{x: tx, y: ty}} | _] = Map.to_list(nodes)

    :ok = Chunk.harvest(chunk, "alice", {tx, ty})
    send(chunk, :flush_db)
    _ = :sys.get_state(chunk)
    assert length(rows(coord)) == 1

    # Wait for in-memory respawn to fire.
    assert wait_until(200, fn ->
             _ = :sys.get_state(chunk)
             Chunk.snapshot(chunk).resource_nodes[id].depleted == false
           end)

    send(chunk, :flush_db)
    _ = :sys.get_state(chunk)
    assert rows(coord) == []
  end

  test "a fresh chunk hydrates Depleted nodes from the repo and respawns when the timer elapses" do
    coord = {9, 9}
    # Pre-seed: a row that says one tree at chunk-centre will respawn in 50ms.
    {cx, cy} = coord
    centre = {cx * 16_000 + 8_000, cy * 16_000 + 8_000}
    {tx, ty} = centre
    soon = DateTime.add(DateTime.utc_now(), 50, :millisecond) |> DateTime.truncate(:microsecond)

    Repo.insert!(%ResourceNode{
      chunk_x: cx,
      chunk_y: cy,
      type: "tree",
      x: tx,
      y: ty,
      depleted_until: soon
    })

    chunk = start_chunk(coord, :chunk3, respawn_ms: 30_000)
    eid = "tree:#{tx}:#{ty}"

    assert Chunk.snapshot(chunk).resource_nodes[eid].depleted == true

    assert wait_until(300, fn ->
             _ = :sys.get_state(chunk)
             Chunk.snapshot(chunk).resource_nodes[eid].depleted == false
           end)
  end

  test "a fresh chunk treats a past-due depletion row as Gatherable" do
    coord = {11, 11}
    {cx, cy} = coord
    {tx, ty} = {cx * 16_000 + 8_000, cy * 16_000 + 8_000}
    past = DateTime.add(DateTime.utc_now(), -60_000, :millisecond) |> DateTime.truncate(:microsecond)

    Repo.insert!(%ResourceNode{
      chunk_x: cx,
      chunk_y: cy,
      type: "tree",
      x: tx,
      y: ty,
      depleted_until: past
    })

    chunk = start_chunk(coord, :chunk4, respawn_ms: 30_000)
    eid = "tree:#{tx}:#{ty}"

    # Past-due rows are ignored by hydration — the Worldgen-seeded
    # Gatherable wins.
    assert Chunk.snapshot(chunk).resource_nodes[eid].depleted == false

    # And the next flush DELETEs the stale row so the table stays clean.
    send(chunk, :flush_db)
    _ = :sys.get_state(chunk)
    assert rows(coord) == []
  end

  defp wait_until(timeout_ms, fun) when timeout_ms <= 0, do: fun.()

  defp wait_until(timeout_ms, fun) do
    if fun.() do
      true
    else
      Process.sleep(5)
      wait_until(timeout_ms - 5, fun)
    end
  end
end
