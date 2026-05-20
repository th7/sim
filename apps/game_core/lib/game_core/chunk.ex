defmodule GameCore.Chunk do
  @moduledoc """
  A Chunk is a fixed-size rectangular partition of the Overworld and the
  unit of process ownership: one GenServer per `{chunk_x, chunk_y}` coord.

  Internals are an ECS over plain maps: a `GameCore.World` holds component
  data, and each tick runs `MovementSystem` then `BroadcastSystem`. Player
  entities use their username as the entity id; non-player entities (added
  in later phases) use integer ids.

  Persistence is delegated to a pluggable `GameCore.ChunkRepo` implementation.
  The default is the `Null` repo (no durability), suitable for tests that
  don't care; production wires `GamePersistence.ChunkRepo`. Players are
  hydrated lazily on `join/2` from `fetch_player/1` and flushed on `leave/2`,
  on a periodic `flush_db` tick, and on chunk terminate.
  """

  use GenServer

  alias GameCore.World
  alias GameCore.Components.{Position, Velocity, Renderable, PlayerControlled}
  alias GameCore.Systems.{MovementSystem, BroadcastSystem}

  @type coord :: {integer(), integer()}
  @type username :: String.t()
  @type intent :: {number(), number()}

  @default_tick_ms 50
  @default_speed 4.0
  @default_flush_ms 5_000
  @default_idle_timeout_ms 5_000

  def start_link(opts) do
    {name, opts} = Keyword.pop(opts, :name)
    gen_opts = if name, do: [name: name], else: []
    start_with_retry(opts, gen_opts, 50)
  end

  # Starting a fresh Chunk under the same `:via` name immediately after the
  # prior owner has died — or is mid-terminate — can race the Registry's
  # asynchronous DOWN handling. If the registered pid is already dead, wait
  # briefly for Registry to clear and retry. If it's still alive but about
  # to die (e.g. mid-terminate from `Supervisor.terminate_child`), monitor
  # it: the moment it exits, retry. Give up only when the deadline elapses
  # with the name still held.
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

  @spec snapshot(GenServer.server()) :: BroadcastSystem.snapshot()
  def snapshot(server), do: GenServer.call(server, :snapshot)

  @spec join(GenServer.server(), username()) :: :ok
  def join(server, username), do: GenServer.call(server, {:join, username})

  @spec leave(GenServer.server(), username()) :: :ok
  def leave(server, username), do: GenServer.call(server, {:leave, username})

  @spec set_intent(GenServer.server(), username(), intent()) :: :ok
  def set_intent(server, username, {dx, dy})
      when is_number(dx) and is_number(dy) do
    GenServer.call(server, {:set_intent, username, {dx * 1.0, dy * 1.0}})
  end

  @spec subscribe(GenServer.server(), pid()) :: :ok
  def subscribe(server, pid), do: GenServer.call(server, {:subscribe, pid})

  @doc """
  Read-only snapshot of this Chunk's runtime state, for the dev-mode overlay.
  Pure read — never mutates state, never blocks the gameplay tick.
  """
  @spec dev_status(GenServer.server()) :: %{
          lifecycle: :hot | :idle_armed,
          idle_ms_remaining: nil | non_neg_integer(),
          entity_count: non_neg_integer(),
          interest_count: non_neg_integer()
        }
  def dev_status(server), do: GenServer.call(server, :dev_status)

  @doc """
  Express interest in keeping this Chunk hot. The chunk monitors `pid`
  and removes it from the interest set on `DOWN`. When the interest set
  is empty for `idle_timeout_ms`, the chunk deactivates (final flush +
  terminate). Phase 6's `GameCore.Session` is the typical caller.
  """
  @spec express_interest(GenServer.server(), pid()) :: :ok
  def express_interest(server, pid), do: GenServer.call(server, {:express_interest, pid})

  @spec release_interest(GenServer.server(), pid()) :: :ok
  def release_interest(server, pid), do: GenServer.call(server, {:release_interest, pid})

  @doc """
  Migration handshake: a source Chunk hands an entity off to its neighbor.
  The destination adds every passed component and triggers an immediate
  out-of-cycle snapshot so observers see the entity in its new chunk
  without waiting for the next broadcast tick.
  """
  @spec migrate_in(GenServer.server(), GameCore.World.eid(), %{module() => any()}) :: :ok
  def migrate_in(server, eid, components) do
    GenServer.call(server, {:migrate_in, eid, components})
  end

  @impl true
  def init(opts) do
    coord = Keyword.fetch!(opts, :coord)
    tick_ms = Keyword.get(opts, :tick_ms, @default_tick_ms)
    speed = Keyword.get(opts, :speed, @default_speed)
    auto_tick = Keyword.get(opts, :auto_tick, true)
    auto_flush = Keyword.get(opts, :auto_flush, true)
    flush_ms = Keyword.get(opts, :flush_ms, @default_flush_ms)
    repo = Keyword.get(opts, :repo, GameCore.ChunkRepo.Null)
    idle_timeout_ms = Keyword.get(opts, :idle_timeout_ms, @default_idle_timeout_ms)

    Process.flag(:trap_exit, true)

    state = %{
      coord: coord,
      world: World.new(),
      tick_ms: tick_ms,
      speed: speed,
      auto_tick: auto_tick,
      auto_flush: auto_flush,
      flush_ms: flush_ms,
      repo: repo,
      subscribers: [],
      tick_count: 0,
      interests: MapSet.new(),
      idle_timeout_ms: idle_timeout_ms,
      idle_since: nil
    }

    if auto_tick, do: schedule_tick(tick_ms)
    if auto_flush, do: schedule_flush(flush_ms)
    {:ok, state}
  end

  @impl true
  def handle_call(:snapshot, _from, state) do
    {:reply, BroadcastSystem.snapshot(state.world), state}
  end

  def handle_call(:dev_status, _from, state) do
    lifecycle = if MapSet.size(state.interests) == 0 and state.idle_since != nil,
                  do: :idle_armed,
                  else: :hot

    idle_ms_remaining =
      case state.idle_since do
        nil ->
          nil

        ts ->
          elapsed = System.monotonic_time(:millisecond) - ts
          max(state.idle_timeout_ms - elapsed, 0)
      end

    entity_count = state.world.components |> Map.get(Position, %{}) |> map_size()

    status = %{
      lifecycle: lifecycle,
      idle_ms_remaining: idle_ms_remaining,
      entity_count: entity_count,
      interest_count: MapSet.size(state.interests)
    }

    {:reply, status, state}
  end

  def handle_call({:join, username}, _from, state) do
    {x, y} = hydrate_position(state, username)

    world =
      state.world
      |> World.add_component(username, Position, %{x: x, y: y})
      |> World.add_component(username, Velocity, %{vx: 0.0, vy: 0.0})
      |> World.add_component(username, Renderable, %{})
      |> World.add_component(username, PlayerControlled, %{})

    {:reply, :ok, %{state | world: world}}
  end

  def handle_call({:leave, username}, _from, state) do
    flush_one(state, username)
    {:reply, :ok, %{state | world: World.remove_entity(state.world, username)}}
  end

  def handle_call({:set_intent, username, {dx, dy}}, _from, state) do
    # No-op if the entity has already migrated to a neighboring chunk;
    # the input is still being routed here from the player's original
    # owner channel (Phase 6 introduces a Session that retargets input).
    case World.fetch(state.world, username, Position) do
      {:ok, _} ->
        world =
          World.add_component(state.world, username, Velocity, %{
            vx: dx * state.speed,
            vy: dy * state.speed
          })

        {:reply, :ok, %{state | world: world}}

      :error ->
        {:reply, :ok, state}
    end
  end

  def handle_call({:subscribe, pid}, _from, state) do
    {:reply, :ok, %{state | subscribers: [pid | state.subscribers]}}
  end

  def handle_call({:express_interest, pid}, _from, state) do
    Process.monitor(pid)
    interests = MapSet.put(state.interests, pid)
    {:reply, :ok, %{state | interests: interests, idle_since: nil}}
  end

  def handle_call({:release_interest, pid}, _from, state) do
    interests = MapSet.delete(state.interests, pid)
    state = %{state | interests: interests}
    {:reply, :ok, maybe_arm_idle(state)}
  end

  def handle_call({:migrate_in, eid, components}, _from, state) do
    world =
      Enum.reduce(components, state.world, fn {mod, data}, w ->
        World.add_component(w, eid, mod, data)
      end)

    snap = BroadcastSystem.snapshot(world)
    Enum.each(state.subscribers, &send(&1, {:snapshot, snap}))

    {:reply, :ok, %{state | world: world}}
  end

  @impl true
  def handle_info(:tick, state) do
    dt = state.tick_ms / 1000.0
    world = MovementSystem.run(state.world, dt)
    {world, migrated?} = migrate_out(world, state.coord, state.repo)
    tick_count = state.tick_count + 1
    state = %{state | world: world, tick_count: tick_count}

    if migrated? or rem(tick_count, 2) == 0 do
      snap = BroadcastSystem.snapshot(world)
      Enum.each(state.subscribers, &send(&1, {:snapshot, snap}))
    end

    if state.auto_tick, do: schedule_tick(state.tick_ms)
    {:noreply, state}
  end

  def handle_info(:flush_db, state) do
    flush_all(state)
    if state.auto_flush, do: schedule_flush(state.flush_ms)
    {:noreply, state}
  end

  def handle_info({:DOWN, _ref, :process, pid, _reason}, state) do
    interests = MapSet.delete(state.interests, pid)
    {:noreply, maybe_arm_idle(%{state | interests: interests})}
  end

  def handle_info(:idle_check, state) do
    if MapSet.size(state.interests) == 0 and state.idle_since != nil and
         System.monotonic_time(:millisecond) - state.idle_since >= state.idle_timeout_ms do
      {:stop, :normal, state}
    else
      {:noreply, state}
    end
  end

  # trap_exit is set so that terminate/2 runs cleanly; ignore EXIT messages
  # from linked test processes (and similar) that aren't our supervisor.
  def handle_info({:EXIT, _from, _reason}, state), do: {:noreply, state}

  @impl true
  def terminate(_reason, state) do
    # Best-effort: at shutdown the repo may already be gone (e.g. its app or
    # an Agent it owns has stopped). Swallow any error so we still terminate.
    try do
      flush_all(state)
    catch
      _, _ -> :ok
    end

    # Drop our Registry entry synchronously: the partition's DOWN handler is
    # async, so without this a successor `start_link` under the same `:via`
    # name can collide with our dead pid for a short window after we exit.
    try do
      Registry.unregister(GameCore.Chunks, state.coord)
    catch
      _, _ -> :ok
    end

    :ok
  end

  defp hydrate_position(state, username) do
    case state.repo.fetch_player(username) do
      %{chunk_x: cx, chunk_y: cy, x: x, y: y} when {cx, cy} == state.coord -> {x, y}
      _ -> chunk_center(state.coord)
    end
  end

  defp chunk_center({cx, cy}) do
    size = GameCore.ChunkGeometry.chunk_size()
    {cx * size + size / 2, cy * size + size / 2}
  end

  defp flush_one(state, username) do
    case World.fetch(state.world, username, Position) do
      {:ok, %{x: x, y: y}} ->
        state.repo.flush_players(state.coord, [%{username: username, x: x, y: y}])

      :error ->
        :ok
    end
  end

  defp flush_all(%{world: world, coord: coord, repo: repo}) do
    positions = Map.get(world.components, Position, %{})
    player_eids = Map.keys(Map.get(world.components, PlayerControlled, %{}))

    players =
      Enum.flat_map(player_eids, fn eid ->
        case Map.fetch(positions, eid) do
          {:ok, %{x: x, y: y}} -> [%{username: eid, x: x, y: y}]
          :error -> []
        end
      end)

    case players do
      [] -> :ok
      _ -> repo.flush_players(coord, players)
    end
  end

  defp schedule_tick(tick_ms), do: Process.send_after(self(), :tick, tick_ms)
  defp schedule_flush(flush_ms), do: Process.send_after(self(), :flush_db, flush_ms)

  defp maybe_arm_idle(state) do
    cond do
      MapSet.size(state.interests) > 0 ->
        %{state | idle_since: nil}

      state.idle_since == nil ->
        Process.send_after(self(), :idle_check, max(state.idle_timeout_ms, 1))
        %{state | idle_since: System.monotonic_time(:millisecond)}

      true ->
        state
    end
  end

  defp migrate_out(world, coord, repo) do
    positions = Map.get(world.components, Position, %{})

    Enum.reduce(positions, {world, false}, fn {eid, %{x: x, y: y}}, {w, migrated?} ->
      case GameCore.ChunkGeometry.coord_for(x, y) do
        ^coord ->
          {w, migrated?}

        dest_coord ->
          {:ok, pid} = GameCore.Chunks.ensure_started(dest_coord, repo)
          :ok = migrate_in(pid, eid, entity_components(w, eid))

          case GameCore.Sessions.whereis(eid) do
            spid when is_pid(spid) -> GameCore.Session.on_migrated(spid, dest_coord)
            _ -> :ok
          end

          {World.remove_entity(w, eid), true}
      end
    end)
  end

  defp entity_components(world, eid) do
    for {mod, m} <- world.components, Map.has_key?(m, eid), into: %{} do
      {mod, Map.fetch!(m, eid)}
    end
  end
end
