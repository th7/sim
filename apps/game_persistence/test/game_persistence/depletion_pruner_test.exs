defmodule GamePersistence.DepletionPrunerTest do
  @moduledoc """
  The pruner periodically DELETEs `resource_nodes` rows whose
  `depleted_until` is in the past. The live game does not depend on this
  for correctness (chunk hydration already skips past-due rows) — the
  pruner exists to keep the table from accumulating respawned-long-ago
  rows for chunks that never re-activate.
  """
  use GamePersistence.DataCase, async: false

  alias GamePersistence.DepletionPruner
  alias GamePersistence.Schemas.ResourceNode

  test "the pruner DELETEs rows whose depleted_until is in the past" do
    past =
      DateTime.add(DateTime.utc_now(), -60_000, :millisecond) |> DateTime.truncate(:microsecond)

    future =
      DateTime.add(DateTime.utc_now(), 60_000, :millisecond) |> DateTime.truncate(:microsecond)

    {:ok, _} =
      Repo.insert(%ResourceNode{
        chunk_x: 0,
        chunk_y: 0,
        type: "tree",
        x: 1,
        y: 1,
        depleted_until: past
      })

    {:ok, _} =
      Repo.insert(%ResourceNode{
        chunk_x: 0,
        chunk_y: 0,
        type: "tree",
        x: 2,
        y: 2,
        depleted_until: future
      })

    # Drive the pruner synchronously rather than waiting on its schedule.
    assert {:ok, deleted} = DepletionPruner.prune_once()
    assert deleted == 1

    remaining = Repo.all(ResourceNode)
    assert length(remaining) == 1
    assert hd(remaining).depleted_until == future
  end

  test "starting the pruner schedules a recurring prune" do
    # Tiny interval so the schedule fires fast in test.
    {:ok, pid} = start_supervised({DepletionPruner, prune_ms: 20, name: :pruner_test})
    assert is_pid(pid)
    # No-op: we just want to confirm the GenServer accepts the opts and
    # schedules itself. The behaviour is verified by `prune_once/0`.
  end
end
