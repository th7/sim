defmodule GameCore.ChunkCase do
  @moduledoc """
  Test case base for tests that activate Chunks via `WarmSet` or
  `Instances`, or that exercise Chunk persistence paths. Provides:

    * a fresh `GamePersistence.Datastore` for each test
    * an Ecto SQL sandbox checkout (so the Datastore's flushes land in
      a per-test isolated transaction)
    * cleanup of any leaked chunks/instances on `on_exit`

  Use either `use GameCore.ChunkCase, async: false`, or — from a test
  that already `use`s another case (e.g. `GameWeb.ChannelCase`) — import
  the helpers and wire them up explicitly:

      import GameCore.ChunkCase, only: [
        datastore_setup: 1,
        reset_chunks_and_instances: 1
      ]
      setup :datastore_setup
      setup :reset_chunks_and_instances
  """

  use ExUnit.CaseTemplate

  using do
    quote do
      import GameCore.ChunkCase,
        only: [datastore_setup: 1, reset_chunks_and_instances: 1]

      setup :datastore_setup
      setup :reset_chunks_and_instances
    end
  end

  @doc """
  Setup hook: checks out an Ecto SQL sandbox owner and starts a fresh
  `GamePersistence.Datastore` under the test supervisor.
  """
  def datastore_setup(tags) do
    pid =
      Ecto.Adapters.SQL.Sandbox.start_owner!(
        GamePersistence.Repo,
        shared: not Map.get(tags, :async, false)
      )

    ExUnit.Callbacks.on_exit(fn ->
      Ecto.Adapters.SQL.Sandbox.stop_owner(pid)
    end)

    ExUnit.Callbacks.start_supervised!(GamePersistence.Datastore)
    :ok
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
