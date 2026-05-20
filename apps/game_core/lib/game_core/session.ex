defmodule GameCore.Session do
  @moduledoc """
  Per-Player GenServer that owns the warm-set of Chunks around the Player.
  Started as a side-car by the owner channel on join; stopped on channel
  terminate. Releases all warm-set interests on terminate; Chunks
  deactivate themselves shortly after their last interested pid disappears.

  The Session is also notified when the Player's entity migrates between
  Chunks (via `on_migrated/2`) so it can pan its warm window to follow.
  """

  # A Session's lifetime is bounded by its owning channel — when the channel
  # dies, the Session is supposed to follow it down. `restart: :temporary`
  # tells `SessionSupervisor` not to bring a Session back after it exits;
  # otherwise the default (`:permanent`) would spawn phantom Sessions that
  # outlive the players they represent and conflict with reconnects.
  use GenServer, restart: :temporary

  alias GameCore.{Chunk, Chunks, Sessions, WarmSet}

  def start_link(opts) do
    username = Keyword.fetch!(opts, :username)
    start_with_retry(opts, [name: Sessions.via(username)], 50)
  end

  # Same Registry-DOWN race as `Chunk.start_link/1` — see that module for the
  # explanation.
  defp start_with_retry(opts, gen_opts, retries_left) do
    case GenServer.start_link(__MODULE__, opts, gen_opts) do
      {:error, {:already_started, pid}} when retries_left > 0 ->
        wait_for_clear(pid, 50)
        start_with_retry(opts, gen_opts, retries_left - 1)

      result ->
        result
    end
  end

  defp wait_for_clear(pid, timeout_ms) do
    if Process.alive?(pid) do
      ref = Process.monitor(pid)

      receive do
        {:DOWN, ^ref, :process, _, _} -> :ok
      after
        timeout_ms ->
          Process.demonitor(ref, [:flush])
          :timeout
      end
    else
      Process.sleep(2)
      :ok
    end
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

    initial_chunk = Keyword.fetch!(opts, :initial_chunk)

    warm_opts =
      Keyword.take(opts, [:repo]) ++
        case Keyword.fetch(opts, :warm_radius) do
          {:ok, r} -> [radius: r]
          :error -> []
        end

    # WarmSet.new/3 warms synchronously, so the Session is fully initialized
    # before start_link/1 returns — see the moduledoc on `GameCore.WarmSet`.
    warm = WarmSet.new(initial_chunk, self(), warm_opts)

    state = %{
      username: Keyword.fetch!(opts, :username),
      current_chunk: initial_chunk,
      warm: warm
    }

    {:ok, state}
  end

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
    {:noreply, %{state | current_chunk: new_coord, warm: WarmSet.recenter(state.warm, new_coord)}}
  end

  @impl true
  def handle_info({:EXIT, _from, _reason}, state) do
    # The owner channel linked to us and has exited — Session lifetime is
    # bounded by the channel's, so follow it down. `terminate/2` will run
    # and release warm-set interests + deregister from Sessions Registry.
    {:stop, :normal, state}
  end

  @impl true
  def terminate(_reason, state) do
    WarmSet.release_all(state.warm)

    # See the same comment in `GameCore.Chunk.terminate/2`.
    safe(fn -> Registry.unregister(Sessions, state.username) end)

    :ok
  end

  defp safe(fun) do
    fun.()
  catch
    _, _ -> :ok
  end
end
