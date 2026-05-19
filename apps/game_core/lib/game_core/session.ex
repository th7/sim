defmodule GameCore.Session do
  @moduledoc """
  Per-Player GenServer that owns the warm-set of Chunks around the Player.
  Started as a side-car by the owner channel on join; stopped on channel
  terminate. Releases all warm-set interests on terminate; Chunks
  deactivate themselves shortly after their last interested pid disappears.

  The Session is also notified when the Player's entity migrates between
  Chunks (via `on_migrated/2`) so it can pan its warm window to follow.
  """

  use GenServer

  alias GameCore.{Chunk, Chunks, Sessions}

  @default_warm_radius 2

  def start_link(opts) do
    username = Keyword.fetch!(opts, :username)
    GenServer.start_link(__MODULE__, opts, name: Sessions.via(username))
  end

  @doc "Called by a Chunk after it migrates a Player's entity to a neighbor."
  @spec on_migrated(GenServer.server(), GameCore.Chunk.coord()) :: :ok
  def on_migrated(server, new_coord), do: GenServer.cast(server, {:migrated, new_coord})

  @spec current_chunk(GenServer.server()) :: GameCore.Chunk.coord()
  def current_chunk(server), do: GenServer.call(server, :current_chunk)

  @doc "Forward input to whichever Chunk currently owns the Player's entity."
  @spec set_intent(GenServer.server(), {number(), number()}) :: :ok
  def set_intent(server, {_, _} = intent), do: GenServer.call(server, {:set_intent, intent})

  @impl true
  def init(opts) do
    Process.flag(:trap_exit, true)

    state = %{
      username: Keyword.fetch!(opts, :username),
      current_chunk: Keyword.fetch!(opts, :initial_chunk),
      repo: Keyword.get(opts, :repo, GameCore.ChunkRepo.Null),
      warm_radius: Keyword.get(opts, :warm_radius, @default_warm_radius),
      warm: MapSet.new()
    }

    {:ok, state, {:continue, :warm_up}}
  end

  @impl true
  def handle_continue(:warm_up, state), do: {:noreply, sync_warm_set(state)}

  @impl true
  def handle_call(:current_chunk, _from, state) do
    {:reply, state.current_chunk, state}
  end

  def handle_call({:set_intent, intent}, _from, state) do
    case Chunks.whereis(state.current_chunk) do
      pid when is_pid(pid) -> Chunk.set_intent(pid, state.username, intent)
      _ -> :ok
    end

    {:reply, :ok, state}
  end

  @impl true
  def handle_cast({:migrated, new_coord}, state) do
    {:noreply, state |> Map.put(:current_chunk, new_coord) |> sync_warm_set()}
  end

  @impl true
  def terminate(_reason, state) do
    for coord <- state.warm do
      case Chunks.whereis(coord) do
        pid when is_pid(pid) -> safe(fn -> Chunk.release_interest(pid, self()) end)
        _ -> :ok
      end
    end

    :ok
  end

  defp sync_warm_set(state) do
    want = window_coords(state.current_chunk, state.warm_radius)
    to_drop = MapSet.difference(state.warm, want)
    to_add = MapSet.difference(want, state.warm)

    for coord <- to_add, do: warm_up(coord, state.repo)

    for coord <- to_drop do
      case Chunks.whereis(coord) do
        pid when is_pid(pid) -> safe(fn -> Chunk.release_interest(pid, self()) end)
        _ -> :ok
      end
    end

    %{state | warm: want}
  end

  # Race between a chunk's idle-deactivation and ensure_started: the lookup
  # can see a pid that's already terminating. Retry once with a fresh start.
  defp warm_up(coord, repo), do: warm_up(coord, repo, 2)

  defp warm_up(_coord, _repo, 0), do: :ok

  defp warm_up(coord, repo, retries) do
    {:ok, pid} = Chunks.ensure_started(coord, repo)

    try do
      Chunk.express_interest(pid, self())
    catch
      :exit, _ -> warm_up(coord, repo, retries - 1)
    end
  end

  defp window_coords({cx, cy}, radius) do
    for dx <- -radius..radius, dy <- -radius..radius, into: MapSet.new() do
      {cx + dx, cy + dy}
    end
  end

  defp safe(fun) do
    fun.()
  catch
    _, _ -> :ok
  end
end
