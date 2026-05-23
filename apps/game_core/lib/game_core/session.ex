defmodule GameCore.Session do
  @moduledoc """
  Per-Player GenServer that owns the Player's chunk-membership lifecycle:
  the realm + current coord of the Player's entity, the Warm set of Chunks
  around them, and final cleanup on disconnect. Started as a side-car by
  the `PlayerChannel` on join; stopped on channel terminate.

  On terminate the Session does `Chunk.leave` on whichever Chunk currently
  owns the entity, then releases all Warm set interests, and — if the
  Player was inside an Instance — eagerly tears down that Instance.

  Realm transitions (`enter_instance/2`, `exit_instance/1`) tear down the
  current Warm set, migrate the entity to the destination Chunk via
  `ChunkMigration`, and build a fresh Warm set in the new realm.
  """

  use GenServer, restart: :temporary

  alias GameCore.{Chunk, Chunks, Instances, InstanceWorldgen, Sessions, WarmSet}

  def start_link(opts) do
    username = Keyword.fetch!(opts, :username)
    start_with_retry(opts, [name: Sessions.via(username)], 50)
  end

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

  @doc """
  Update the Session's record of where the Player's entity lives now —
  used by `ChunkMigration` after a Boundary crossing within the current
  realm. Pans the Warm set to the new center.
  """
  @spec relocate(GenServer.server(), GameCore.Chunk.coord()) :: :ok
  def relocate(server, new_coord), do: GenServer.cast(server, {:relocate, new_coord})

  @spec current_chunk(GenServer.server()) :: GameCore.Chunk.coord()
  def current_chunk(server), do: GenServer.call(server, :current_chunk)

  @doc "Read the Session's current realm — `:overworld` or `{:instance, id}`."
  @spec current_realm(GenServer.server()) :: Chunks.realm()
  def current_realm(server), do: GenServer.call(server, :current_realm)

  @doc "Forward input to whichever Chunk currently owns the Player's entity."
  @spec set_intent(GenServer.server(), {number(), number()}) :: :ok
  def set_intent(server, {_, _} = intent), do: GenServer.call(server, {:set_intent, intent})

  @doc """
  Enter an Instance from an Overworld Portal at `portal_pos`. Spawns a
  fresh Instance, migrates the Player's entity to its center chunk
  spawn-offset, rebuilds the Warm set, and caches the return point.
  """
  @spec enter_instance(GenServer.server(), GameCore.Chunk.coord(), {integer(), integer()}) ::
          :ok | {:error, atom()}
  def enter_instance(server, from_coord, {px, py})
      when is_integer(px) and is_integer(py) do
    GenServer.call(server, {:enter_instance, from_coord, {px, py}})
  end

  @doc """
  Exit the current Instance — migrate the entity back to the cached
  Overworld return Chunk + Portal cell + offset, rebuild the Warm set,
  and terminate the Instance.
  """
  @spec exit_instance(GenServer.server()) :: :ok | {:error, atom()}
  def exit_instance(server), do: GenServer.call(server, :exit_instance)

  @spec harvest(GenServer.server(), {integer(), integer()}) :: :ok | {:error, atom()}
  def harvest(server, {x, y}) when is_integer(x) and is_integer(y),
    do: GenServer.call(server, {:harvest, {x, y}})

  @spec build(GenServer.server(), atom(), {integer(), integer()}) :: :ok | {:error, atom()}
  def build(server, type, {x, y}) when is_atom(type) and is_integer(x) and is_integer(y),
    do: GenServer.call(server, {:build, type, {x, y}})

  @spec damage(GenServer.server(), {integer(), integer()}) :: :ok | {:error, atom()}
  def damage(server, {x, y}) when is_integer(x) and is_integer(y),
    do: GenServer.call(server, {:damage, {x, y}})

  @impl true
  def init(opts) do
    Process.flag(:trap_exit, true)

    initial_chunk = Keyword.fetch!(opts, :initial_chunk)
    username = Keyword.fetch!(opts, :username)

    warm_opts =
      [realm: :overworld] ++
        case Keyword.fetch(opts, :warm_radius) do
          {:ok, r} -> [radius: r]
          :error -> []
        end

    warm = WarmSet.new(initial_chunk, self(), warm_opts)

    case Chunks.whereis(:overworld, initial_chunk) do
      pid when is_pid(pid) -> Chunk.join(pid, username)
      _ -> :ok
    end

    state = %{
      username: username,
      realm: :overworld,
      current_chunk: initial_chunk,
      warm: warm,
      return_to: nil
    }

    {:ok, state}
  end

  @impl true
  def handle_call(:current_chunk, _from, state) do
    {:reply, state.current_chunk, state}
  end

  def handle_call(:current_realm, _from, state) do
    {:reply, state.realm, state}
  end

  def handle_call({:set_intent, intent}, _from, state) do
    case Chunks.whereis(state.realm, state.current_chunk) do
      pid when is_pid(pid) -> Chunk.set_intent(pid, state.username, intent)
      _ -> :ok
    end

    {:reply, :ok, state}
  end

  def handle_call({:harvest, coords}, _from, state) do
    reply = forward_to_current(state, &Chunk.harvest(&1, state.username, coords))
    {:reply, reply, state}
  end

  def handle_call({:build, _type, _coords}, _from, %{realm: {:instance, _}} = state) do
    {:reply, {:error, :no_build_in_instance}, state}
  end

  def handle_call({:build, type, coords}, _from, state) do
    reply = forward_to_current(state, &Chunk.build(&1, state.username, type, coords))
    {:reply, reply, state}
  end

  def handle_call({:damage, coords}, _from, state) do
    reply = forward_to_current(state, &Chunk.damage(&1, state.username, coords))
    {:reply, reply, state}
  end

  def handle_call({:enter_instance, from_coord, portal_pos}, _from, %{realm: :overworld} = state) do
    {:ok, id} = Instances.start_new()
    new_realm = {:instance, id}
    center_coord = {1, 1}

    new_state =
      transition_realm(state,
        src_realm: :overworld,
        src_coord: from_coord,
        dst_realm: new_realm,
        dst_coord: center_coord,
        spawn_pos: instance_spawn_pos(),
        save_pos: offset_from_portal(portal_pos),
        warm_radius: 1,
        return_to: {:overworld, from_coord, portal_pos}
      )

    {:reply, :ok, new_state}
  end

  def handle_call({:enter_instance, _, _}, _from, state) do
    {:reply, {:error, :already_in_instance}, state}
  end

  def handle_call(
        :exit_instance,
        _from,
        %{realm: {:instance, id}, return_to: {:overworld, dest_coord, portal_pos}} = state
      ) do
    spawn_pos = offset_from_portal(portal_pos)

    new_state =
      transition_realm(state,
        src_realm: state.realm,
        src_coord: state.current_chunk,
        dst_realm: :overworld,
        dst_coord: dest_coord,
        spawn_pos: spawn_pos,
        # Source is Instance (no emission) — save_pos is ignored, kept uniform.
        save_pos: spawn_pos,
        warm_radius: nil,
        return_to: nil
      )

    :ok = Instances.terminate(id)
    {:reply, :ok, new_state}
  end

  def handle_call(:exit_instance, _from, state) do
    {:reply, {:error, :not_in_instance}, state}
  end

  # Cross-realm transition shared by enter_instance / exit_instance: pull the
  # entity out of `src_realm/src_coord` with a save flush + Position override,
  # drop it into `dst_realm/dst_coord`, swap the realm-scoped WarmSet, and
  # publish a `relocated` event. Returns the new Session state.
  defp transition_realm(state, opts) do
    src_realm = Keyword.fetch!(opts, :src_realm)
    src_coord = Keyword.fetch!(opts, :src_coord)
    dst_realm = Keyword.fetch!(opts, :dst_realm)
    dst_coord = Keyword.fetch!(opts, :dst_coord)
    spawn_pos = Keyword.fetch!(opts, :spawn_pos)
    save_pos = Keyword.fetch!(opts, :save_pos)
    radius = Keyword.fetch!(opts, :warm_radius)
    return_to = Keyword.fetch!(opts, :return_to)

    case Chunks.whereis(src_realm, src_coord) do
      src_pid when is_pid(src_pid) ->
        components = Chunk.take_components_for(src_pid, state.username, spawn_pos, save_pos)
        {:ok, dest_pid} = Chunks.ensure_started(dst_realm, dst_coord)
        :ok = Chunk.migrate_in(dest_pid, state.username, components)

      _ ->
        :ok
    end

    WarmSet.release_all(state.warm)

    warm_opts =
      [realm: dst_realm] ++ if radius, do: [radius: radius], else: []

    new_warm = WarmSet.new(dst_coord, self(), warm_opts)

    new_state = %{
      state
      | realm: dst_realm,
        current_chunk: dst_coord,
        warm: new_warm,
        return_to: return_to
    }

    publish_relocated(new_state)
    new_state
  end

  defp forward_to_current(state, fun) do
    case Chunks.whereis(state.realm, state.current_chunk) do
      pid when is_pid(pid) -> fun.(pid)
      _ -> {:error, :no_chunk}
    end
  end

  @impl true
  def handle_cast({:relocate, new_coord}, state) do
    {:noreply, %{state | current_chunk: new_coord, warm: WarmSet.recenter(state.warm, new_coord)}}
  end

  @impl true
  def handle_info({:EXIT, _from, _reason}, state) do
    {:stop, :normal, state}
  end

  @impl true
  def terminate(_reason, state) do
    case Chunks.whereis(state.realm, state.current_chunk) do
      pid when is_pid(pid) -> safe(fn -> Chunk.leave(pid, state.username) end)
      _ -> :ok
    end

    WarmSet.release_all(state.warm)

    case state.realm do
      {:instance, id} -> safe(fn -> Instances.terminate(id) end)
      _ -> :ok
    end

    safe(fn -> Registry.unregister(Sessions, state.username) end)

    :ok
  end

  defp safe(fun) do
    fun.()
  catch
    _, _ -> :ok
  end

  # Spawn position one world unit (1000 sub-units) west of the return-Portal,
  # so the Player doesn't immediately overlap it and bounce straight back out.
  defp instance_spawn_pos do
    {px, py} = InstanceWorldgen.return_portal_pos()
    {px - 1000, py}
  end

  # On Instance exit, re-emerge one world unit west of the entry Portal in
  # the Overworld — symmetric with `instance_spawn_pos`.
  defp offset_from_portal({px, py}), do: {px - 1000, py}

  # Notify the owner PlayerChannel that the Player has changed realm/chunk,
  # so the client can cycle its snapshot subscriptions to the new realm's
  # topics. Per-Player PubSub topic mirrors `self:<username>`.
  defp publish_relocated(state) do
    payload = %{
      realm: serialize_realm(state.realm),
      coord: Tuple.to_list(state.current_chunk)
    }

    Phoenix.PubSub.broadcast(
      GameCore.PubSub,
      "player_events:#{state.username}",
      {:relocated, payload}
    )
  end

  defp serialize_realm(:overworld), do: %{kind: "overworld"}
  defp serialize_realm({:instance, id}), do: %{kind: "instance", id: id}
end
