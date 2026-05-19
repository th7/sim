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
    world =
      World.add_component(state.world, username, Velocity, %{
        vx: dx * state.speed,
        vy: dy * state.speed
      })

    {:reply, :ok, %{state | world: world}}
  end

  def handle_call({:subscribe, pid}, _from, state) do
    {:reply, :ok, %{state | subscribers: [pid | state.subscribers]}}
  end

  @impl true
  def handle_info(:tick, state) do
    dt = state.tick_ms / 1000.0
    world = MovementSystem.run(state.world, dt)
    tick_count = state.tick_count + 1
    state = %{state | world: world, tick_count: tick_count}

    if rem(tick_count, 2) == 0 do
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
      _ -> {0.0, 0.0}
    end
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
end
