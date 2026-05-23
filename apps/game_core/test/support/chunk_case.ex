defmodule GameCore.ChunkCase do
  @moduledoc """
  Test case base for tests that activate Chunks via `WarmSet` or
  `Instances` and need to tear them down between tests. The Session's
  warm set populates `GameCore.ChunkSupervisor` with 25 chunks per
  Session; without explicit cleanup those leak across tests and trip
  `start_supervised!` with `:already_started`.

  Use either via `use GameCore.ChunkCase, async: false`, or — from a
  test that already `use`s another case (e.g. `GameWeb.ChannelCase`) —
  call `GameCore.ChunkCase.reset_chunks_and_instances/1` directly:

      setup :reset_chunks_and_instances
      import GameCore.ChunkCase, only: [reset_chunks_and_instances: 1]
  """

  use ExUnit.CaseTemplate

  using do
    quote do
      import GameCore.ChunkCase, only: [reset_chunks_and_instances: 1]
      setup :reset_chunks_and_instances
    end
  end

  @doc """
  Setup hook: registers an `on_exit` that terminates every child under
  `GameCore.ChunkSupervisor` and `GameCore.InstancesSupervisor`.
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
