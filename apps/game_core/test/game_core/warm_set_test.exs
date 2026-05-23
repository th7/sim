defmodule GameCore.WarmSetTest do
  use GameCore.ChunkCase, async: false

  alias GameCore.{Chunk, Chunks, WarmSet}

  test "new/3 activates every coord in the window and records them as members" do
    ws = WarmSet.new({0, 0}, self(), radius: 1)

    expected = for cx <- -1..1, cy <- -1..1, into: MapSet.new(), do: {cx, cy}
    assert WarmSet.members(ws) == expected

    for coord <- expected do
      pid = Chunks.whereis(:overworld, coord)
      assert is_pid(pid), "expected chunk #{inspect(coord)} to be activated"
      assert Chunk.dev_status(pid).interest_count >= 1
    end
  end

  test "recenter/2 activates new column, releases old, updates members" do
    ws = WarmSet.new({0, 0}, self(), radius: 1)
    dropped = for cy <- -1..1, do: {-1, cy}
    added = for cy <- -1..1, do: {2, cy}

    ws = WarmSet.recenter(ws, {1, 0})

    expected = for cx <- 0..2, cy <- -1..1, into: MapSet.new(), do: {cx, cy}
    assert WarmSet.members(ws) == expected

    for coord <- dropped do
      pid = Chunks.whereis(:overworld, coord)
      # Chunk may already be terminating with no other holders; if still alive,
      # our interest must be gone.
      if is_pid(pid), do: assert(Chunk.dev_status(pid).interest_count == 0)
    end

    for coord <- added do
      pid = Chunks.whereis(:overworld, coord)
      assert is_pid(pid), "expected chunk #{inspect(coord)} to be activated"
      assert Chunk.dev_status(pid).interest_count >= 1
    end
  end

  test "release_all/1 drops every member interest and empties the set" do
    ws = WarmSet.new({0, 0}, self(), radius: 1)
    warmed = for coord <- WarmSet.members(ws), do: {coord, Chunks.whereis(:overworld, coord)}

    ws = WarmSet.release_all(ws)
    assert WarmSet.members(ws) == MapSet.new()

    for {coord, pid} <- warmed, is_pid(pid) do
      assert Chunk.dev_status(pid).interest_count == 0,
             "expected interest on #{inspect(coord)} to be released"
    end
  end
end
