defmodule GameCore.Chunk do
  @moduledoc """
  A Chunk is a fixed-size rectangular partition of the Overworld and the
  unit of process ownership: one GenServer per `{chunk_x, chunk_y}` coord.

  Internals are an ECS over plain maps: a `GameCore.World` holds component
  data, and each tick runs `MovementSystem` then `BroadcastSystem`. Player
  entities use their username as the entity id; non-player entities (added
  in later phases) use integer ids.
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

    state = %{
      coord: coord,
      world: World.new(),
      tick_ms: tick_ms,
      speed: speed,
      auto_tick: auto_tick,
      subscribers: [],
      tick_count: 0
    }

    if auto_tick, do: schedule_tick(tick_ms)
    {:ok, state}
  end

  @impl true
  def handle_call(:snapshot, _from, state) do
    {:reply, BroadcastSystem.snapshot(state.world), state}
  end

  def handle_call({:join, username}, _from, state) do
    world =
      state.world
      |> World.add_component(username, Position, %{x: 0.0, y: 0.0})
      |> World.add_component(username, Velocity, %{vx: 0.0, vy: 0.0})
      |> World.add_component(username, Renderable, %{})
      |> World.add_component(username, PlayerControlled, %{})

    {:reply, :ok, %{state | world: world}}
  end

  def handle_call({:leave, username}, _from, state) do
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

  defp schedule_tick(tick_ms), do: Process.send_after(self(), :tick, tick_ms)
end
