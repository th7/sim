defmodule GameCore.Instances do
  @moduledoc """
  Lifecycle of Instances — ephemeral, private 3×3 chunk grids spawned on
  demand for a Player overlapping a Portal. Each Instance gets its own
  `DynamicSupervisor` whose only children are the 9 Instance Chunks; the
  per-Instance supervisor in turn lives under the top-level
  `GameCore.InstancesSupervisor`. Identity is a monotonic positive
  integer; chunks are registered in the shared `GameCore.Chunks` Registry
  under `{{:instance, id}, {cx, cy}}` keys, distinct from Overworld keys
  by the realm tag.

  Termination is one call: `terminate/1` stops the per-Instance
  supervisor, all 9 chunks die in lockstep, and Registry entries clean
  up automatically.
  """

  @doc """
  Spawn a new Instance: per-Instance supervisor + 9 Chunks (3×3) with
  the Null repo. Returns the Instance's monotonic id.
  """
  @spec start_new() :: {:ok, integer()}
  def start_new do
    id = System.unique_integer([:positive, :monotonic])
    realm = {:instance, id}

    children =
      for cx <- 0..2, cy <- 0..2 do
        coord = {cx, cy}

        Supervisor.child_spec(
          {GameCore.Chunk,
           realm: realm,
           coord: coord,
           repo: GameCore.ChunkRepo.Null,
           name: GameCore.Chunks.via(realm, coord)},
          id: {:chunk, coord}
        )
      end

    sup_name = {:via, Registry, {GameCore.InstanceSupervisors, id}}

    {:ok, _sup} =
      DynamicSupervisor.start_child(
        GameCore.InstancesSupervisor,
        %{
          id: realm,
          start:
            {Supervisor, :start_link, [children, [strategy: :one_for_all, name: sup_name]]},
          type: :supervisor,
          restart: :temporary
        }
      )

    {:ok, id}
  end

  @doc """
  Stop the per-Instance supervisor for `id`. All 9 chunks exit; Registry
  entries clean up via their DOWN handlers. Synchronous: returns once
  all chunks have stopped and their Registry keys are clear.
  """
  @spec terminate(integer()) :: :ok
  def terminate(id) when is_integer(id) do
    realm = {:instance, id}

    case find_supervisor(realm) do
      nil ->
        :ok

      sup_pid ->
        ref = Process.monitor(sup_pid)
        :ok = DynamicSupervisor.terminate_child(GameCore.InstancesSupervisor, sup_pid)

        receive do
          {:DOWN, ^ref, :process, ^sup_pid, _} -> :ok
        after
          1_000 -> Process.demonitor(ref, [:flush])
        end

        wait_for_registry_clear(realm, 50)
        :ok
    end
  end

  defp find_supervisor({:instance, id}) do
    case Registry.lookup(GameCore.InstanceSupervisors, id) do
      [{pid, _}] -> pid
      [] -> nil
    end
  end

  defp wait_for_registry_clear(_realm, 0), do: :ok

  defp wait_for_registry_clear(realm, retries) do
    any_alive? =
      Enum.any?(for cx <- 0..2, cy <- 0..2, do: GameCore.Chunks.whereis(realm, {cx, cy}))

    if any_alive? do
      Process.sleep(2)
      wait_for_registry_clear(realm, retries - 1)
    else
      :ok
    end
  end
end
