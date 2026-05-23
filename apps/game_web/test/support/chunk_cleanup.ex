defmodule GameWeb.ChunkCleanup do
  @moduledoc """
  Setup hook for channel tests that activate Chunks via Session/WarmSet.
  Equivalent to `GameCore.ChunkCase`'s `reset_chunks_and_instances/1`, but
  reachable from game_web (game_core's test/support isn't on this app's
  test codepath).

  Use:

      import GameWeb.ChunkCleanup, only: [reset_chunks_and_instances: 1]
      setup :reset_chunks_and_instances
  """

  @doc """
  `on_exit` hook tearing down every child of `GameCore.ChunkSupervisor`
  and `GameCore.InstancesSupervisor`.
  """
  def reset_chunks_and_instances(_context) do
    ExUnit.Callbacks.on_exit(fn ->
      for {_, pid, _, _} <- DynamicSupervisor.which_children(GameCore.ChunkSupervisor),
          is_pid(pid) do
        DynamicSupervisor.terminate_child(GameCore.ChunkSupervisor, pid)
      end

      for {_, pid, _, _} <- DynamicSupervisor.which_children(GameCore.InstancesSupervisor),
          is_pid(pid) do
        DynamicSupervisor.terminate_child(GameCore.InstancesSupervisor, pid)
      end
    end)

    :ok
  end
end
