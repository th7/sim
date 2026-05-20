defmodule GameCore.SessionsTest do
  use ExUnit.Case, async: false

  alias GameCore.{Session, Sessions}

  setup do
    on_exit(fn ->
      for {_, pid, _, _} <- DynamicSupervisor.which_children(GameCore.ChunkSupervisor) do
        DynamicSupervisor.terminate_child(GameCore.ChunkSupervisor, pid)
      end
    end)

    :ok
  end

  test "count/0 returns the number of running Sessions" do
    before = Sessions.count()

    {:ok, a} = Session.start_link(username: "alice", initial_chunk: {0, 0}, warm_radius: 0)
    {:ok, b} = Session.start_link(username: "bob", initial_chunk: {0, 0}, warm_radius: 0)

    assert Sessions.count() == before + 2

    a_ref = Process.monitor(a)
    b_ref = Process.monitor(b)
    GenServer.stop(a)
    GenServer.stop(b)
    assert_receive {:DOWN, ^a_ref, :process, _, _}
    assert_receive {:DOWN, ^b_ref, :process, _, _}

    # Registry processes the DOWN message asynchronously after the process
    # exits — wait for the deregistration to land.
    Stream.repeatedly(fn -> Sessions.count() end)
    |> Enum.find(fn count -> count == before end)

    assert Sessions.count() == before
  end
end
