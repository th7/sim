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

  def start_link(opts) do
    {name, opts} = Keyword.pop(opts, :name)
    gen_opts = if name, do: [name: name], else: []
    GenServer.start_link(__MODULE__, opts, gen_opts)
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
      tick_count: 0
    }

    if auto_tick, do: schedule_tick(tick_ms)
    if auto_flush, do: schedule_flush(flush_ms)
    {:ok, state}
  end

  @impl true
  def handle_call(:snapshot, _from, state) do
    {:reply, BroadcastSystem.snapshot(state.world), state}
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
    {world, migrated?} = migrate_out(world, state.coord)
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

  @impl true
  def terminate(_reason, state) do
    # Best-effort: at shutdown the repo may already be gone (e.g. its app or
    # an Agent it owns has stopped). Swallow any error so we still terminate.
    try do
      flush_all(state)
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

  defp migrate_out(world, coord) do
    positions = Map.get(world.components, Position, %{})

    Enum.reduce(positions, {world, false}, fn {eid, %{x: x, y: y}}, {w, migrated?} ->
      case GameCore.ChunkGeometry.coord_for(x, y) do
        ^coord ->
          {w, migrated?}

        dest_coord ->
          case GameCore.Chunks.whereis(dest_coord) do
            pid when is_pid(pid) ->
              :ok = migrate_in(pid, eid, entity_components(w, eid))
              {World.remove_entity(w, eid), true}

            _ ->
              # No destination available (outside the live grid). Leave the
              # entity in place; Phase 5 doesn't try to expand the world.
              {w, migrated?}
          end
      end
    end)
  end

  defp entity_components(world, eid) do
    for {mod, m} <- world.components, Map.has_key?(m, eid), into: %{} do
      {mod, Map.fetch!(m, eid)}
    end
  end
end
