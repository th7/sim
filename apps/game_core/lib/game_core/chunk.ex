defmodule GameCore.Chunk do
  @moduledoc """
  A Chunk is a fixed-size rectangular partition of the Overworld and the
  unit of process ownership: one GenServer per `{chunk_x, chunk_y}` coordinate.
  In Phase 1 there is exactly one Chunk at `{0, 0}` and Player state lives in
  a plain map on the GenServer state.
  """

  use GenServer

  @type coord :: {integer(), integer()}
  @type username :: String.t()
  @type intent :: {float(), float()}
  @type snapshot :: %{players: %{username() => %{x: float(), y: float()}}}

  @default_tick_ms 50
  @default_speed 4.0

  def start_link(opts) do
    {name, opts} = Keyword.pop(opts, :name)
    gen_opts = if name, do: [name: name], else: []
    GenServer.start_link(__MODULE__, opts, gen_opts)
  end

  @spec snapshot(GenServer.server()) :: snapshot()
  def snapshot(server), do: GenServer.call(server, :snapshot)

  @spec join(GenServer.server(), username()) :: :ok
  def join(server, username), do: GenServer.call(server, {:join, username})

  @spec leave(GenServer.server(), username()) :: :ok
  def leave(server, username), do: GenServer.call(server, {:leave, username})

  @spec set_intent(GenServer.server(), username(), intent()) :: :ok
  def set_intent(server, username, {dx, dy} = _intent)
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
      players: %{},
      intents: %{},
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
    {:reply, %{players: state.players}, state}
  end

  def handle_call({:join, username}, _from, state) do
    players = Map.put(state.players, username, %{x: 0.0, y: 0.0})
    intents = Map.put(state.intents, username, {0.0, 0.0})
    {:reply, :ok, %{state | players: players, intents: intents}}
  end

  def handle_call({:leave, username}, _from, state) do
    {:reply, :ok,
     %{state | players: Map.delete(state.players, username), intents: Map.delete(state.intents, username)}}
  end

  def handle_call({:set_intent, username, intent}, _from, state) do
    {:reply, :ok, %{state | intents: Map.put(state.intents, username, intent)}}
  end

  def handle_call({:subscribe, pid}, _from, state) do
    {:reply, :ok, %{state | subscribers: [pid | state.subscribers]}}
  end

  @impl true
  def handle_info(:tick, state) do
    dt = state.tick_ms / 1000.0
    players = step(state.players, state.intents, state.speed, dt)
    tick_count = state.tick_count + 1
    state = %{state | players: players, tick_count: tick_count}

    if rem(tick_count, 2) == 0 do
      snap = %{players: players}
      Enum.each(state.subscribers, &send(&1, {:snapshot, snap}))
    end

    if state.auto_tick, do: schedule_tick(state.tick_ms)
    {:noreply, state}
  end

  defp step(players, intents, speed, dt) do
    Map.new(players, fn {username, %{x: x, y: y}} ->
      {dx, dy} = Map.get(intents, username, {0.0, 0.0})
      {username, %{x: x + dx * speed * dt, y: y + dy * speed * dt}}
    end)
  end

  defp schedule_tick(tick_ms), do: Process.send_after(self(), :tick, tick_ms)
end
