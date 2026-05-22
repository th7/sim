defmodule GamePersistence.DepletionPruner do
  @moduledoc """
  Background GenServer that periodically DELETEs `resource_nodes` rows
  whose `depleted_until` is in the past. The live game does not depend
  on this for correctness — chunk hydration already skips past-due rows
  — but without it the table grows unboundedly for chunks that never
  re-activate.

  Default cadence is 60 seconds, override via the `prune_ms` opt or the
  `:depletion_prune_ms` application env.
  """

  use GenServer

  import Ecto.Query

  alias GamePersistence.Repo
  alias GamePersistence.Schemas.ResourceNode

  @default_prune_ms 60_000

  def start_link(opts \\ []) do
    name = Keyword.get(opts, :name, __MODULE__)
    GenServer.start_link(__MODULE__, opts, name: name)
  end

  @doc """
  Run one prune pass against the global `Repo`. Synchronous; intended
  for tests and on-demand sweeps. Returns `{:ok, deleted_count}`.
  """
  @spec prune_once() :: {:ok, non_neg_integer()}
  def prune_once do
    now = DateTime.utc_now()

    {count, _} =
      Repo.delete_all(
        from(r in ResourceNode,
          where: not is_nil(r.depleted_until) and r.depleted_until < ^now
        )
      )

    {:ok, count}
  end

  @impl true
  def init(opts) do
    prune_ms =
      Keyword.get(opts, :prune_ms) ||
        Application.get_env(:game_persistence, :depletion_prune_ms, @default_prune_ms)

    schedule(prune_ms)
    {:ok, %{prune_ms: prune_ms}}
  end

  @impl true
  def handle_info(:prune, state) do
    {:ok, _} = prune_once()
    schedule(state.prune_ms)
    {:noreply, state}
  end

  defp schedule(ms), do: Process.send_after(self(), :prune, ms)
end
