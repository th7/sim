defmodule GameCore.ChunkLifecycleTest do
  use ExUnit.Case, async: false

  alias GameCore.Chunk
  alias GameCore.Chunks

  test "ensure_started returns the same pid the second time" do
    {:ok, pid1} = Chunks.ensure_started({100, 200})
    {:ok, pid2} = Chunks.ensure_started({100, 200})
    assert pid1 == pid2

    # Cleanup
    Process.exit(pid1, :shutdown)
  end

  test "a chunk with at least one interested pid stays alive" do
    {:ok, pid} = Chunks.ensure_started({101, 0})
    ref = Process.monitor(pid)

    Chunk.express_interest(pid, self())

    # Drive an idle check; should NOT trigger termination because there's
    # still an interested pid.
    send(pid, :idle_check)
    _ = :sys.get_state(pid)
    refute_received {:DOWN, ^ref, :process, _, _}

    Chunk.release_interest(pid, self())
    Process.exit(pid, :shutdown)
  end

  test "a chunk with no interested pids terminates after the idle window" do
    {:ok, pid} =
      DynamicSupervisor.start_child(
        GameCore.ChunkSupervisor,
        {Chunk,
         coord: {102, 0},
         name: Chunks.via({102, 0}),
         auto_tick: false,
         auto_flush: false,
         idle_timeout_ms: 0}
      )

    ref = Process.monitor(pid)
    Chunk.express_interest(pid, self())
    Chunk.release_interest(pid, self())

    # With idle_timeout_ms: 0, the next :idle_check terminates immediately.
    send(pid, :idle_check)
    assert_receive {:DOWN, ^ref, :process, _, _}
  end

  test "an interested pid dying releases the interest" do
    {:ok, pid} = Chunks.ensure_started({103, 0})

    {:ok, transient} = Agent.start(fn -> :ok end)
    Chunk.express_interest(pid, transient)
    Agent.stop(transient)

    # Give the chunk a moment to process the DOWN message.
    _ = :sys.get_state(pid)
    assert Chunk.dev_status(pid).interest_count == 0

    Process.exit(pid, :shutdown)
  end
end
