defmodule GameCore.Chunk do
  @moduledoc """
  A Chunk is a fixed-size rectangular partition of either the **Overworld**
  or an **Instance**, and the unit of process ownership: one GenServer per
  `{realm, {chunk_x, chunk_y}}` key in the shared `GameCore.Chunks`
  Registry. Overworld chunks live under `realm = :overworld`; Instance
  chunks under `realm = {:instance, id}`.

  Internals are an ECS over plain maps: a `GameCore.World` holds component
  data, and each tick runs `MovementSystem` then a Portal-overlap check
  then `BroadcastSystem`. Player entities use their username as the entity
  id; non-player entities (Resource nodes, Structures, Portals) use string
  ids derived from their type and position.

  Persistence is delegated to a pluggable `GameCore.ChunkRepo`
  implementation. The default `Null` repo (no durability) is used for
  tests and for Instance chunks (Instance state is in-memory only);
  Overworld chunks in production use `GamePersistence.ChunkRepo`. Players
  are hydrated lazily on `join/2` from `fetch_player/1` and flushed on
  `leave/2`, on a periodic `flush_db` tick, and on chunk terminate. The
  cross-realm `take_components_for/4` also forces a flush so a mid-realm
  disconnect persists a sensible Overworld return position.

  Snapshot broadcasts go to a realm-prefixed PubSub topic
  (`chunk:cx:cy` for Overworld, `instance:<id>:chunk:cx:cy` for Instance),
  ensuring subscribers in different realms can't see each other's state.
  """

  # `:transient` is load-bearing: a Chunk's idle deactivation is an
  # intentional `:normal` exit (see ChunkLifecycle and CONTEXT.md). With
  # the default `:permanent`, the DynamicSupervisor restarts every
  # deactivated Chunk — and a single player moving one chunk releases
  # interest on a whole column of (2 * radius + 1) chunks at once. They
  # all idle out together, trip ChunkSupervisor's max_restarts, the
  # supervisor crashes, the outer Supervisor resurrects an empty one,
  # every live chunk dies, and every Session is stranded on a dead
  # current_chunk — silently losing movement.
  use GenServer, restart: :transient

  alias GameCore.{ChunkLifecycle, ChunkMigration, Worldgen, World}

  alias GameCore.Components.{
    Depleted,
    Gatherable,
    Inventory,
    PlayerControlled,
    Portal,
    Position,
    Renderable,
    Structure,
    Velocity
  }

  alias GameCore.Structure.Catalogue

  alias GameCore.Systems.{MovementSystem, BroadcastSystem}

  @type coord :: {integer(), integer()}
  @type username :: String.t()
  @type intent :: {number(), number()}

  @default_tick_ms 50
  # 4 world units/sec = 4000 sub-units/sec (positions are in sub-units).
  @default_speed 4_000.0
  @default_flush_ms 5_000
  @default_respawn_ms 30_000

  # 1.0 world unit, squared, in sub-units. Used by all interact verbs.
  @interact_range_sq 1_000 * 1_000

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

  @spec harvest(GenServer.server(), username(), {integer(), integer()}) ::
          :ok | {:error, :too_far | :no_target | :depleted}
  def harvest(server, username, {x, y}) when is_integer(x) and is_integer(y) do
    GenServer.call(server, {:harvest, username, {x, y}})
  end

  @doc "Read-only query for a Player's current Inventory. Empty map if none."
  @spec player_inventory(GenServer.server(), username()) :: %{atom() => non_neg_integer()}
  def player_inventory(server, username),
    do: GenServer.call(server, {:player_inventory, username})

  @doc """
  Admin/test-only: overwrite a Player's Inventory in this Chunk. Production
  paths mutate Inventory only through `harvest` / `build` / `damage`.
  """
  @spec set_inventory(GenServer.server(), username(), %{atom() => non_neg_integer()}) :: :ok
  def set_inventory(server, username, items) when is_map(items),
    do: GenServer.call(server, {:set_inventory, username, items})

  @spec build(GenServer.server(), username(), atom(), {integer(), integer()}) ::
          :ok | {:error, atom()}
  def build(server, username, type, {x, y})
      when is_atom(type) and is_integer(x) and is_integer(y) do
    GenServer.call(server, {:build, username, type, {x, y}})
  end

  @damage_per_click 25

  @spec damage(GenServer.server(), username(), {integer(), integer()}) ::
          :ok | {:error, atom()}
  def damage(server, username, {x, y}) when is_integer(x) and is_integer(y) do
    GenServer.call(server, {:damage, username, {x, y}})
  end

  @doc """
  Read-only diagnostic of this Chunk's runtime state — lifecycle, idle
  countdown, entity count, interest count. Pure read; never mutates state
  or blocks the gameplay tick. Used by the dev-mode overlay and by tests
  inspecting Chunk lifecycle through the public interface.
  """
  @spec dev_status(GenServer.server()) :: %{
          lifecycle: :hot | :idle_armed,
          idle_ms_remaining: nil | non_neg_integer(),
          entity_count: non_neg_integer(),
          interest_count: non_neg_integer()
        }
  def dev_status(server), do: GenServer.call(server, :dev_status)

  # Interest tracking is the low-level mechanism behind `GameCore.WarmSet`.
  # Callers wanting to keep a Chunk hot should construct a WarmSet rather
  # than calling these directly; they are documented here for the WarmSet
  # implementation and for Chunk-lifecycle tests.
  @doc false
  @spec express_interest(GenServer.server(), pid()) :: :ok
  def express_interest(server, pid), do: GenServer.call(server, {:express_interest, pid})

  @doc false
  @spec release_interest(GenServer.server(), pid()) :: :ok
  def release_interest(server, pid), do: GenServer.call(server, {:release_interest, pid})

  # Destination side of the Boundary crossing handshake. Adds every passed
  # component and triggers an out-of-cycle snapshot so observers see the
  # entity in its new chunk without waiting for the next broadcast tick.
  # Callers handing off entities between chunks should go through
  # `GameCore.ChunkMigration.cross/5`; this is its low-level destination
  # call.
  @doc false
  @spec migrate_in(GenServer.server(), GameCore.World.eid(), %{module() => any()}) :: :ok
  def migrate_in(server, eid, components) do
    GenServer.call(server, {:migrate_in, eid, components})
  end

  @doc """
  Source side of a cross-realm migration. Removes `eid` from this Chunk's
  world and returns its component map with `Position` overridden to
  `dest_pos` (so the destination realm sees the entity at the spawn point,
  not at its pre-migration coords in a different coordinate space). Before
  removal, flushes `save_pos` to this Chunk's persistence — so a mid-realm
  disconnect lands the Player back at `save_pos` on reconnect, not at the
  Portal cell that would immediately re-trigger entry.

  Returns `%{}` if the entity is unknown.
  """
  @spec take_components_for(
          GenServer.server(),
          GameCore.World.eid(),
          {integer(), integer()},
          {integer(), integer()}
        ) :: %{module() => any()}
  def take_components_for(server, eid, dest_pos, save_pos)
      when is_tuple(dest_pos) and is_tuple(save_pos) do
    GenServer.call(server, {:take_components_for, eid, dest_pos, save_pos})
  end

  @impl true
  def init(opts) do
    coord = Keyword.fetch!(opts, :coord)
    realm = Keyword.get(opts, :realm, :overworld)
    tick_ms = Keyword.get(opts, :tick_ms, @default_tick_ms)
    speed = Keyword.get(opts, :speed, @default_speed)
    auto_tick = Keyword.get(opts, :auto_tick, true)
    auto_flush = Keyword.get(opts, :auto_flush, true)
    flush_ms = Keyword.get(opts, :flush_ms, @default_flush_ms)
    respawn_ms = Keyword.get(opts, :respawn_ms, @default_respawn_ms)
    repo = Keyword.get(opts, :repo, GameCore.ChunkRepo.Null)

    Process.flag(:trap_exit, true)

    world =
      World.new()
      |> seed_resource_nodes(coord)
      |> seed_structures(coord, repo)
      |> hydrate_depletions(coord, repo)
      |> seed_portals(realm, coord)

    state = %{
      coord: coord,
      realm: realm,
      world: world,
      tick_ms: tick_ms,
      speed: speed,
      auto_tick: auto_tick,
      auto_flush: auto_flush,
      flush_ms: flush_ms,
      respawn_ms: respawn_ms,
      repo: repo,
      tick_count: 0,
      lifecycle: ChunkLifecycle.new(Keyword.take(opts, [:idle_timeout_ms]))
    }

    if auto_tick, do: schedule_tick(tick_ms)
    if auto_flush, do: schedule_flush(flush_ms)
    {:ok, state}
  end

  defp hydrate_depletions(world, coord, repo) do
    now = DateTime.utc_now()

    Enum.reduce(repo.fetch_depletions(coord), world, fn d, w ->
      case DateTime.compare(d.depleted_until, now) do
        :gt ->
          eid = node_eid(d.type, d.x, d.y)
          remaining_ms = DateTime.diff(d.depleted_until, now, :millisecond)
          Process.send_after(self(), {:respawn, eid}, remaining_ms)

          w
          |> remove_component(Gatherable, eid)
          |> World.add_component(eid, Depleted, %{
            type: d.type,
            depleted_until: d.depleted_until
          })

        _ ->
          # Past-due: leave the Worldgen-seeded Gatherable in place. The
          # next flush_db will DELETE the stale row.
          w
      end
    end)
  end

  defp seed_resource_nodes(world, coord) do
    Enum.reduce(Worldgen.resource_nodes(coord), world, fn %{type: type, x: x, y: y}, w ->
      eid = node_eid(type, x, y)

      w
      |> World.add_component(eid, Position, %{x: x, y: y})
      |> World.add_component(eid, Renderable, %{})
      |> World.add_component(eid, Gatherable, %{type: type, yields: yield_for(type)})
    end)
  end

  defp node_eid(type, x, y), do: "#{type}:#{x}:#{y}"

  defp seed_portals(world, realm, coord) do
    portals =
      case realm do
        :overworld -> Worldgen.portals(coord)
        {:instance, _} -> GameCore.InstanceWorldgen.portals(coord)
      end

    Enum.reduce(portals, world, fn %{type: type, direction: dir, x: x, y: y}, w ->
      eid = portal_eid(type, x, y)

      w
      |> World.add_component(eid, Position, %{x: x, y: y})
      |> World.add_component(eid, Renderable, %{})
      |> World.add_component(eid, Portal, %Portal{type: type, direction: dir})
    end)
  end

  defp portal_eid(type, x, y), do: "portal:#{type}:#{x}:#{y}"

  # Overworld is unbounded — `nil` lets `MovementSystem` skip clamping and
  # rely on `migrate_out` for chunk-edge handling. Instances are bounded
  # to their 3×3 grid; movement past the perimeter clamps.
  defp movement_bounds(:overworld), do: nil

  defp movement_bounds({:instance, _}) do
    size = GameCore.ChunkGeometry.chunk_size()
    {0, 0, 3 * size, 3 * size}
  end

  defp seed_structures(world, coord, repo) do
    Enum.reduce(repo.fetch_structures(coord), world, fn s, w ->
      eid = structure_eid(s.id)

      w
      |> World.add_component(eid, Position, %{x: s.x, y: s.y})
      |> World.add_component(eid, Renderable, %{})
      |> World.add_component(eid, Structure, %{type: s.type, owner: s.owner, hp: s.hp})
    end)
  end

  defp yield_for(:tree), do: :wood

  defp check_in_range(px, py, tx, ty) do
    dx = px - tx
    dy = py - ty

    if dx * dx + dy * dy <= @interact_range_sq do
      :ok
    else
      {:error, :too_far}
    end
  end

  defp player_pos(world, username) do
    case World.fetch(world, username, Position) do
      {:ok, %{x: x, y: y}} -> {:ok, {x, y}}
      :error -> {:error, :no_player}
    end
  end

  defp player_inv(world, username) do
    case World.fetch(world, username, Inventory) do
      {:ok, %{items: items}} -> {:ok, items}
      :error -> {:error, :no_player}
    end
  end

  defp fetch_gatherable(world, eid) do
    case World.fetch(world, eid, Gatherable) do
      {:ok, data} ->
        {:ok, data}

      :error ->
        case World.fetch(world, eid, Depleted) do
          {:ok, _} -> {:error, :depleted}
          :error -> {:error, :no_target}
        end
    end
  end

  defp remove_component(%World{components: cs} = world, mod, eid) do
    case Map.fetch(cs, mod) do
      {:ok, inner} ->
        %{world | components: Map.put(cs, mod, Map.delete(inner, eid))}

      :error ->
        world
    end
  end

  defp ok_or(true, _reason), do: :ok
  defp ok_or(false, reason), do: {:error, reason}

  defp check_in_chunk(coord, x, y) do
    if GameCore.ChunkGeometry.coord_for(x, y) == coord do
      :ok
    else
      {:error, :out_of_chunk}
    end
  end

  defp check_cell_empty(%World{components: cs}, x, y) do
    structures = Map.get(cs, Structure, %{})
    positions = Map.get(cs, Position, %{})

    collision? =
      Enum.any?(structures, fn {eid, _} ->
        case Map.fetch(positions, eid) do
          {:ok, %{x: ^x, y: ^y}} -> true
          _ -> false
        end
      end)

    if collision?, do: {:error, :cell_occupied}, else: :ok
  end

  defp subtract_cost(items, cost) do
    Enum.reduce_while(cost, {:ok, items}, fn {item, qty}, {:ok, acc} ->
      case Map.get(acc, item, 0) do
        n when n >= qty -> {:cont, {:ok, Map.put(acc, item, n - qty)}}
        _ -> {:halt, {:error, :insufficient_materials}}
      end
    end)
  end

  defp structure_eid(id) when is_integer(id), do: Integer.to_string(id)
  defp structure_eid(id) when is_binary(id), do: id

  defp publish_self(username, items) do
    Phoenix.PubSub.broadcast(
      GameCore.PubSub,
      "self:#{username}",
      {:self, %{inventory: items}}
    )
  end

  defp find_structure_at(%World{components: cs}, x, y) do
    structs = Map.get(cs, Structure, %{})
    positions = Map.get(cs, Position, %{})

    Enum.find_value(structs, {:error, :no_target}, fn {eid, data} ->
      case Map.fetch(positions, eid) do
        {:ok, %{x: ^x, y: ^y}} -> {:ok, {eid, data}}
        _ -> false
      end
    end)
  end

  @impl true
  def handle_call(:snapshot, _from, state) do
    {:reply, BroadcastSystem.snapshot(state.world), state}
  end

  def handle_call(:dev_status, _from, state) do
    entity_count = state.world.components |> Map.get(Position, %{}) |> map_size()
    status = Map.put(ChunkLifecycle.dev_view(state.lifecycle), :entity_count, entity_count)
    {:reply, status, state}
  end

  def handle_call({:join, username}, _from, state) do
    {x, y, items} = hydrate_player(state, username)

    world =
      state.world
      |> World.add_component(username, Position, %{x: x, y: y})
      |> World.add_component(username, Velocity, %{vx: 0.0, vy: 0.0})
      |> World.add_component(username, Renderable, %{})
      |> World.add_component(username, PlayerControlled, %{})
      |> World.add_component(username, Inventory, %{items: items})

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

  def handle_call({:express_interest, pid}, _from, state) do
    {:reply, :ok, %{state | lifecycle: ChunkLifecycle.express(state.lifecycle, pid)}}
  end

  def handle_call({:release_interest, pid}, _from, state) do
    {:reply, :ok, %{state | lifecycle: ChunkLifecycle.release(state.lifecycle, pid)}}
  end

  def handle_call({:harvest, username, {tx, ty}}, _from, state) do
    eid = node_eid(:tree, tx, ty)

    with {:ok, {px, py}} <- player_pos(state.world, username),
         :ok <- check_in_range(px, py, tx, ty),
         {:ok, %{type: gtype, yields: item}} <- fetch_gatherable(state.world, eid),
         {:ok, items} <- player_inv(state.world, username) do
      new_items = Map.update(items, item, 1, &(&1 + 1))
      depleted_until = DateTime.add(DateTime.utc_now(), state.respawn_ms, :millisecond)

      world =
        state.world
        |> World.add_component(username, Inventory, %{items: new_items})
        |> remove_component(Gatherable, eid)
        |> World.add_component(eid, Depleted, %{type: gtype, depleted_until: depleted_until})

      Process.send_after(self(), {:respawn, eid}, state.respawn_ms)

      publish_self(username, new_items)
      {:reply, :ok, %{state | world: world}}
    else
      {:error, reason} -> {:reply, {:error, reason}, state}
    end
  end

  def handle_call({:player_inventory, username}, _from, state) do
    items =
      case World.fetch(state.world, username, Inventory) do
        {:ok, %{items: items}} -> items
        :error -> %{}
      end

    {:reply, items, state}
  end

  def handle_call({:set_inventory, username, items}, _from, state) do
    case World.fetch(state.world, username, Inventory) do
      {:ok, _} ->
        world = World.add_component(state.world, username, Inventory, %{items: items})
        publish_self(username, items)
        {:reply, :ok, %{state | world: world}}

      :error ->
        {:reply, {:error, :no_player}, state}
    end
  end

  def handle_call({:damage, username, {x, y}}, _from, state) do
    with {:ok, {px, py}} <- player_pos(state.world, username),
         :ok <- check_in_range(px, py, x, y),
         {:ok, {eid, struct}} <- find_structure_at(state.world, x, y) do
      new_hp = struct.hp - @damage_per_click

      if new_hp > 0 do
        world =
          World.add_component(state.world, eid, Structure, %{struct | hp: new_hp})

        {:reply, :ok, %{state | world: world}}
      else
        with {sid, ""} <- Integer.parse(eid),
             :ok <- state.repo.destroy_structure(sid) do
          world = World.remove_entity(state.world, eid)
          {:reply, :ok, %{state | world: world}}
        else
          _ -> {:reply, {:error, :destroy_failed}, state}
        end
      end
    else
      {:error, reason} -> {:reply, {:error, reason}, state}
    end
  end

  def handle_call({:build, username, type, {x, y}}, _from, state) do
    with :ok <- Catalogue.valid?(type) |> ok_or(:invalid_type),
         :ok <- check_in_chunk(state.coord, x, y),
         :ok <- check_cell_empty(state.world, x, y),
         {:ok, items} <- player_inv(state.world, username),
         {:ok, new_items} <- subtract_cost(items, Catalogue.cost(type)),
         {:ok, sid} <- state.repo.build_structure(state.coord, username, type, x, y, new_items) do
      eid = structure_eid(sid)

      world =
        state.world
        |> World.add_component(username, Inventory, %{items: new_items})
        |> World.add_component(eid, Position, %{x: x, y: y})
        |> World.add_component(eid, Renderable, %{})
        |> World.add_component(eid, Structure, %{
          type: type,
          owner: username,
          hp: Catalogue.max_hp(type)
        })

      publish_self(username, new_items)
      {:reply, :ok, %{state | world: world}}
    else
      {:error, reason} -> {:reply, {:error, reason}, state}
    end
  end

  def handle_call({:migrate_in, eid, components}, _from, state) do
    world =
      Enum.reduce(components, state.world, fn {mod, data}, w ->
        World.add_component(w, eid, mod, data)
      end)

    snap = BroadcastSystem.snapshot(world)
    broadcast_snapshot(state, snap)

    {:reply, :ok, %{state | world: world}}
  end

  def handle_call({:take_components_for, eid, {dx, dy}, {sx, sy}}, _from, state) do
    components =
      case Map.get(state.world.components, Position) do
        %{^eid => _} ->
          # Persist `save_pos` rather than current Position: on Overworld
          # entry, that means saving the post-exit-offset cell (so a mid-
          # Instance disconnect reconnects there, not on top of the Portal
          # they just stepped onto — which would loop back into the Instance).
          # No-op on Instance source (Null repo).
          world_with_save = World.add_component(state.world, eid, Position, %{x: sx, y: sy})
          flush_one(%{state | world: world_with_save}, eid)

          collect_components(state.world, eid)
          |> Map.put(Position, %{x: dx, y: dy})
          |> Map.put(Velocity, %{vx: 0.0, vy: 0.0})

        _ ->
          %{}
      end

    new_world = World.remove_entity(state.world, eid)
    snap = BroadcastSystem.snapshot(new_world)
    broadcast_snapshot(state, snap)

    {:reply, components, %{state | world: new_world}}
  end

  defp collect_components(%World{components: components}, eid) do
    Enum.reduce(components, %{}, fn {mod, by_eid}, acc ->
      case Map.fetch(by_eid, eid) do
        {:ok, data} -> Map.put(acc, mod, data)
        :error -> acc
      end
    end)
  end

  defp broadcast_snapshot(state, snap) do
    topic = chunk_topic(state.realm, state.coord)
    Phoenix.PubSub.broadcast(GameCore.PubSub, topic, {:snapshot, snap})
  end

  defp chunk_topic(:overworld, {cx, cy}), do: "chunk:#{cx}:#{cy}"
  defp chunk_topic({:instance, id}, {cx, cy}), do: "instance:#{id}:chunk:#{cx}:#{cy}"

  @impl true
  def handle_info(:tick, state) do
    dt = state.tick_ms / 1000.0
    world = MovementSystem.run(state.world, dt, bounds: movement_bounds(state.realm))
    {world, migrated?} = migrate_out(world, state.realm, state.coord, state.repo)
    state = %{state | world: world, tick_count: state.tick_count + 1}

    check_portal_overlaps(state)

    if migrated? or rem(state.tick_count, 2) == 0 do
      snap = BroadcastSystem.snapshot(state.world)
      broadcast_snapshot(state, snap)
    end

    if state.auto_tick, do: schedule_tick(state.tick_ms)
    {:noreply, state}
  end

  def handle_info(:flush_db, state) do
    flush_all(state)
    flush_depletions(state)
    if state.auto_flush, do: schedule_flush(state.flush_ms)
    {:noreply, state}
  end

  def handle_info({:respawn, eid}, state) do
    case World.fetch(state.world, eid, Depleted) do
      {:ok, %{type: type}} ->
        world =
          state.world
          |> remove_component(Depleted, eid)
          |> World.add_component(eid, Gatherable, %{type: type, yields: yield_for(type)})

        snap = BroadcastSystem.snapshot(world)
        broadcast_snapshot(state, snap)

        {:noreply, %{state | world: world}}

      :error ->
        {:noreply, state}
    end
  end

  def handle_info({:DOWN, _ref, :process, pid, _reason}, state) do
    {:noreply, %{state | lifecycle: ChunkLifecycle.handle_down(state.lifecycle, pid)}}
  end

  def handle_info(:idle_check, state) do
    case ChunkLifecycle.check_idle(state.lifecycle) do
      {:deactivate, _} -> {:stop, :normal, state}
      {:keep, lc} -> {:noreply, %{state | lifecycle: lc}}
    end
  end

  # trap_exit is set so that terminate/2 runs cleanly. A `:shutdown` EXIT
  # is the standard supervisor-shutdown protocol — propagate as :normal stop
  # so the chunk dies and Registry cleanup runs. Other EXITs (e.g. a linked
  # test process exiting cleanly) are non-fatal.
  def handle_info({:EXIT, _from, :shutdown}, state), do: {:stop, :normal, state}
  def handle_info({:EXIT, _from, _reason}, state), do: {:noreply, state}

  # Portal-overlap range: 0.5 world units squared = 250_000 sub-unit² — close
  # enough that "stepping on the portal" feels like the trigger, not a wide
  # proximity check.
  @portal_overlap_range_sq 250_000

  defp check_portal_overlaps(state) do
    portals = Map.get(state.world.components, Portal, %{})

    if map_size(portals) > 0 do
      players = Map.get(state.world.components, PlayerControlled, %{})
      positions = Map.get(state.world.components, Position, %{})

      # Portal and Player entities always have Position (added together in
      # seed_portals and handle_call({:join, ...})), so the inner Map.fetch!
      # is total — a missing Position would be a programmer error.
      for {portal_eid, %{direction: dir}} <- portals,
          %{x: portal_x, y: portal_y} = Map.fetch!(positions, portal_eid),
          {username, _} <- players,
          %{x: px, y: py} = Map.fetch!(positions, username),
          dx = px - portal_x,
          dy = py - portal_y,
          dx * dx + dy * dy <= @portal_overlap_range_sq do
        trigger_portal(dir, username, state)
      end
    end
  end

  defp trigger_portal(:into_instance, username, state) do
    case GameCore.Sessions.whereis(username) do
      pid when is_pid(pid) ->
        portal_pos = first_portal_pos(state)
        spawn_caller(fn -> GameCore.Session.enter_instance(pid, state.coord, portal_pos) end)

      _ ->
        :ok
    end
  end

  defp trigger_portal(:out_of_instance, username, _state) do
    case GameCore.Sessions.whereis(username) do
      pid when is_pid(pid) -> spawn_caller(fn -> GameCore.Session.exit_instance(pid) end)
      _ -> :ok
    end
  end

  defp first_portal_pos(state) do
    positions = Map.get(state.world.components, Position, %{})
    portals = Map.get(state.world.components, Portal, %{})
    {eid, _} = Enum.at(portals, 0)
    %{x: x, y: y} = Map.fetch!(positions, eid)
    {x, y}
  end

  # Fire-and-forget: an unlinked process makes the synchronous Session call.
  # Keeps the tick loop responsive and avoids any chance of deadlock if the
  # Session calls back into this Chunk during the transition.
  defp spawn_caller(fun), do: spawn(fun)

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
      Registry.unregister(GameCore.Chunks, {state.realm, state.coord})
    catch
      _, _ -> :ok
    end

    :ok
  end

  defp hydrate_player(state, username) do
    case state.repo.fetch_player(username) do
      %{chunk_x: cx, chunk_y: cy, x: x, y: y} = saved when {cx, cy} == state.coord ->
        {x, y, Map.get(saved, :inventory, %{})}

      saved ->
        {cx, cy} = chunk_center(state.coord)
        items = if is_map(saved), do: Map.get(saved, :inventory, %{}), else: %{}
        {cx, cy, items}
    end
  end

  defp chunk_center({cx, cy}) do
    size = GameCore.ChunkGeometry.chunk_size()
    half = div(size, 2)
    {cx * size + half, cy * size + half}
  end

  defp flush_one(state, username) do
    case World.fetch(state.world, username, Position) do
      {:ok, %{x: x, y: y}} ->
        items = player_items(state.world, username)

        state.repo.flush_players(state.coord, [
          %{username: username, x: x, y: y, inventory: items}
        ])

      :error ->
        :ok
    end
  end

  defp flush_depletions(%{world: world, coord: coord, repo: repo}) do
    positions = Map.get(world.components, Position, %{})
    depleteds = Map.get(world.components, Depleted, %{})

    rows =
      for {eid, %{type: type, depleted_until: %DateTime{} = until}} <- depleteds,
          {:ok, %{x: x, y: y}} <- [Map.fetch(positions, eid)] do
        %{type: type, x: x, y: y, depleted_until: until}
      end

    repo.flush_depletions(coord, rows)
  end

  defp flush_all(%{world: world, coord: coord, repo: repo}) do
    positions = Map.get(world.components, Position, %{})
    player_eids = Map.keys(Map.get(world.components, PlayerControlled, %{}))

    players =
      Enum.flat_map(player_eids, fn eid ->
        case Map.fetch(positions, eid) do
          {:ok, %{x: x, y: y}} ->
            [%{username: eid, x: x, y: y, inventory: player_items(world, eid)}]

          :error ->
            []
        end
      end)

    case players do
      [] -> :ok
      _ -> repo.flush_players(coord, players)
    end
  end

  defp player_items(world, username) do
    case World.fetch(world, username, Inventory) do
      {:ok, %{items: items}} -> items
      :error -> %{}
    end
  end

  defp schedule_tick(tick_ms), do: Process.send_after(self(), :tick, tick_ms)
  defp schedule_flush(flush_ms), do: Process.send_after(self(), :flush_db, flush_ms)

  defp migrate_out(world, realm, coord, repo) do
    positions = Map.get(world.components, Position, %{})

    Enum.reduce(positions, {world, false}, fn {eid, %{x: x, y: y}}, {w, migrated?} ->
      case GameCore.ChunkGeometry.coord_for(x, y) do
        ^coord ->
          {w, migrated?}

        dest_coord ->
          :ok =
            ChunkMigration.cross(realm, eid, coord, dest_coord, entity_components(w, eid), repo)

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
