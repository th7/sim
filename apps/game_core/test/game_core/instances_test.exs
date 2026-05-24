defmodule GameCore.InstancesTest do
  use GameCore.ChunkCase, async: false

  alias GameCore.{Chunk, Chunks, Instances}

  test "start_new/0 spawns 9 Instance chunks (3×3) under a fresh supervisor" do
    {:ok, id} = Instances.start_new()

    for cx <- 0..2, cy <- 0..2 do
      assert is_pid(Chunks.whereis({:instance, id}, {cx, cy})),
             "expected Instance chunk #{cx},#{cy} to be alive"
    end

    :ok = Instances.terminate(id)
  end

  test "terminate/1 stops all 9 Instance chunks and clears their Registry keys" do
    {:ok, id} = Instances.start_new()
    :ok = Instances.terminate(id)

    # Synchronize: Registry cleanup completes as supervisor children exit.
    for cx <- 0..2, cy <- 0..2 do
      refute Chunks.whereis({:instance, id}, {cx, cy}),
             "expected Instance chunk #{cx},#{cy} to be gone"
    end
  end

  test "start_new/0 returns a unique id every time" do
    {:ok, id1} = Instances.start_new()
    {:ok, id2} = Instances.start_new()

    assert id1 != id2

    :ok = Instances.terminate(id1)
    :ok = Instances.terminate(id2)
  end

  test "Instance chunks contain no Worldgen Resource nodes" do
    # Worldgen.resource_nodes/1 places a 5-tree cluster at every chunk
    # center — including the return-Portal cell of Instance chunk {1, 1}.
    # Per CONTEXT.md, Instances have no Resource nodes; the seed must skip
    # them so the return Portal is reachable (no tree Footprint blocking).
    {:ok, id} = Instances.start_new()
    centre = Chunks.whereis({:instance, id}, {1, 1})

    %{resource_nodes: nodes} = Chunk.snapshot(centre)
    assert nodes == %{}

    :ok = Instances.terminate(id)
  end
end
