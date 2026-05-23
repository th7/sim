defmodule GamePersistence.Datastore do
  @moduledoc """
  Single-node persistence chokepoint. Chunks emit state changes here and
  read durable state through here. See `DESIGN.md` and `CONTEXT.md` —
  this is the **Datastore**, **pending writes**, **backpressure** domain.
  """

  use GenServer

  import Ecto.Query, only: [from: 2]

  @default_n_high 1_000
  @default_n_low 200
  @default_t_high_ms 30_000
  @default_t_low_ms 5_000

  def start_link(opts \\ []) do
    {name, opts} = Keyword.pop(opts, :name, __MODULE__)
    GenServer.start_link(__MODULE__, opts, name: name)
  end

  def upsert_player(username, {chunk_x, chunk_y}, x, y, inventory)
      when is_binary(username) and is_integer(chunk_x) and is_integer(chunk_y) and
             is_integer(x) and is_integer(y) and is_map(inventory) do
    GenServer.call(__MODULE__, {:upsert_player, username, {chunk_x, chunk_y}, x, y, inventory})
  end

  def fetch_player(username) when is_binary(username) do
    GenServer.call(__MODULE__, {:fetch_player, username})
  end

  def upsert_structure({_, _} = coord, owner, type, x, y, hp)
      when is_binary(owner) and is_atom(type) and is_integer(x) and is_integer(y) and
             is_integer(hp) do
    GenServer.call(__MODULE__, {:upsert_structure, coord, owner, type, x, y, hp})
  end

  def fetch_structures({_, _} = coord) do
    GenServer.call(__MODULE__, {:fetch_structures, coord})
  end

  def delete_structure(x, y) when is_integer(x) and is_integer(y) do
    GenServer.call(__MODULE__, {:delete_structure, x, y})
  end

  def upsert_depletion(realm, {_, _} = coord, type, x, y, %DateTime{} = depleted_until)
      when is_atom(type) and is_integer(x) and is_integer(y) do
    GenServer.call(
      __MODULE__,
      {:upsert_depletion, realm, coord, type, x, y, depleted_until}
    )
  end

  def delete_depletion(realm, {_, _} = coord, type, x, y)
      when is_atom(type) and is_integer(x) and is_integer(y) do
    GenServer.call(__MODULE__, {:delete_depletion, realm, coord, type, x, y})
  end

  def fetch_depletions(realm, {_, _} = coord) do
    GenServer.call(__MODULE__, {:fetch_depletions, realm, coord})
  end

  @doc "Synchronous flush of pending writes. Returns `:ok` on success."
  def flush_now(server \\ __MODULE__) do
    GenServer.call(server, :flush_now)
  end

  @doc false
  def dump_pending(server \\ __MODULE__) do
    GenServer.call(server, :dump_pending)
  end

  @doc false
  def mode(server \\ __MODULE__) do
    GenServer.call(server, :mode)
  end

  @impl true
  def init(opts) do
    {:ok,
     %{
       pending: %{player: %{}, structure: %{}, depletion: %{}},
       pending_at: %{},
       repo: Keyword.get(opts, :repo, GamePersistence.Repo),
       mode: :flowing,
       parked: [],
       n_high: Keyword.get(opts, :n_high, @default_n_high),
       n_low: Keyword.get(opts, :n_low, @default_n_low),
       t_high_ms: Keyword.get(opts, :t_high_ms, @default_t_high_ms),
       t_low_ms: Keyword.get(opts, :t_low_ms, @default_t_low_ms)
     }}
  end

  @impl true
  def terminate(_reason, state) do
    # One final flush on graceful shutdown. Swallow errors — we're going
    # down anyway, and the alternative is a noisier shutdown log.
    try do
      do_flush(state)
    catch
      _, _ -> :ok
    end

    :ok
  end

  # --- Write ops (subject to backpressure) ---

  @impl true
  def handle_call({:upsert_player, _, _, _, _, _} = op, from, state),
    do: apply_or_park(state, from, op)

  def handle_call({:upsert_structure, _, _, _, _, _, _} = op, from, state),
    do: apply_or_park(state, from, op)

  def handle_call({:delete_structure, _, _} = op, from, state),
    do: apply_or_park(state, from, op)

  def handle_call({:upsert_depletion, _, _, _, _, _, _} = op, from, state),
    do: apply_or_park(state, from, op)

  def handle_call({:delete_depletion, _, _, _, _, _} = op, from, state),
    do: apply_or_park(state, from, op)

  # --- Reads (bypass backpressure) ---

  def handle_call({:fetch_player, username}, _from, state) do
    case Map.get(state.pending.player, username) do
      nil -> {:reply, fetch_player_from_db(state.repo, username), state}
      entry -> {:reply, entry, state}
    end
  end

  def handle_call({:fetch_structures, coord}, _from, state) do
    results =
      for {{x, y}, entry} <- state.pending.structure,
          entry != :tombstone,
          GameCore.ChunkGeometry.coord_for(x, y) == coord,
          do: entry

    {:reply, results, state}
  end

  def handle_call({:fetch_depletions, realm, coord}, _from, state) do
    {:reply, merged_depletions(state, realm, coord), state}
  end

  # --- Operational ---

  def handle_call(:flush_now, _from, state) do
    case do_flush(state) do
      {:ok, state} ->
        state =
          state
          |> maybe_disengage_backpressure()
          |> drain_parked()

        {:reply, :ok, state}

      {:error, _} = err ->
        {:reply, err, state}
    end
  end

  def handle_call(:dump_pending, _from, state), do: {:reply, state.pending, state}
  def handle_call(:mode, _from, state), do: {:reply, state.mode, state}

  # --- apply / park ---

  defp apply_or_park(%{mode: :backpressured} = state, from, op) do
    {:noreply, %{state | parked: state.parked ++ [{from, op}]}}
  end

  defp apply_or_park(state, _from, op) do
    state =
      state
      |> apply_op(op)
      |> maybe_engage_backpressure()

    {:reply, :ok, state}
  end

  defp apply_op(state, op) do
    %{
      state
      | pending: apply_to_pending(state.pending, op),
        pending_at: track_timestamp(state.pending_at, op_key(op))
    }
  end

  defp apply_to_pending(pending, {:upsert_player, username, {cx, cy}, x, y, inventory}) do
    entry = %{username: username, chunk_x: cx, chunk_y: cy, x: x, y: y, inventory: inventory}
    %{pending | player: Map.put(pending.player, username, entry)}
  end

  defp apply_to_pending(pending, {:upsert_structure, coord, owner, type, x, y, hp}) do
    entry = %{coord: coord, type: type, owner: owner, x: x, y: y, hp: hp}
    %{pending | structure: Map.put(pending.structure, {x, y}, entry)}
  end

  defp apply_to_pending(pending, {:delete_structure, x, y}) do
    %{pending | structure: Map.put(pending.structure, {x, y}, :tombstone)}
  end

  defp apply_to_pending(pending, {:upsert_depletion, realm, coord, type, x, y, until}) do
    entry = %{type: type, x: x, y: y, depleted_until: until}
    %{pending | depletion: Map.put(pending.depletion, {realm, coord, type, x, y}, entry)}
  end

  defp apply_to_pending(pending, {:delete_depletion, realm, coord, type, x, y}) do
    %{pending | depletion: Map.put(pending.depletion, {realm, coord, type, x, y}, :tombstone)}
  end

  defp op_key({:upsert_player, username, _, _, _, _}), do: {:player, username}
  defp op_key({:upsert_structure, _coord, _owner, _type, x, y, _hp}), do: {:structure, {x, y}}
  defp op_key({:delete_structure, x, y}), do: {:structure, {x, y}}

  defp op_key({:upsert_depletion, realm, coord, type, x, y, _until}),
    do: {:depletion, {realm, coord, type, x, y}}

  defp op_key({:delete_depletion, realm, coord, type, x, y}),
    do: {:depletion, {realm, coord, type, x, y}}

  defp track_timestamp(pending_at, key) do
    if Map.has_key?(pending_at, key),
      do: pending_at,
      else: Map.put(pending_at, key, System.monotonic_time(:millisecond))
  end

  defp oldest_age_ms(%{pending_at: at}) when map_size(at) == 0, do: 0

  defp oldest_age_ms(%{pending_at: at}) do
    System.monotonic_time(:millisecond) - Enum.min(Map.values(at))
  end

  # --- Backpressure mode transitions ---

  defp pending_size(state) do
    map_size(state.pending.player) +
      map_size(state.pending.structure) +
      map_size(state.pending.depletion)
  end

  defp maybe_engage_backpressure(%{mode: :flowing} = state) do
    if pending_size(state) >= state.n_high or oldest_age_ms(state) >= state.t_high_ms do
      %{state | mode: :backpressured}
    else
      state
    end
  end

  defp maybe_engage_backpressure(state), do: state

  defp maybe_disengage_backpressure(%{mode: :backpressured} = state) do
    if pending_size(state) < state.n_low and oldest_age_ms(state) < state.t_low_ms do
      %{state | mode: :flowing}
    else
      state
    end
  end

  defp maybe_disengage_backpressure(state), do: state

  defp drain_parked(%{mode: :backpressured} = state), do: state
  defp drain_parked(%{parked: []} = state), do: state

  defp drain_parked(%{parked: [{from, op} | rest]} = state) do
    state = apply_op(%{state | parked: rest}, op)
    GenServer.reply(from, :ok)

    state
    |> maybe_engage_backpressure()
    |> drain_parked()
  end

  # --- Flush ---

  defp do_flush(state) do
    state.repo.transaction(fn ->
      Enum.each(state.pending.player, fn {_username, entry} ->
        upsert_player_row(state.repo, entry)
      end)

      Enum.each(state.pending.structure, fn
        {{x, y}, :tombstone} -> delete_structure_row(state.repo, x, y)
        {{_x, _y}, entry} -> upsert_structure_row(state.repo, entry)
      end)

      Enum.each(state.pending.depletion, fn
        {{_realm, coord, type, x, y}, :tombstone} ->
          delete_depletion_row(state.repo, coord, type, x, y)

        {{_realm, coord, _type, _x, _y}, entry} ->
          upsert_depletion_row(state.repo, coord, entry)
      end)
    end)
    |> case do
      {:ok, _} -> {:ok, %{state | pending: empty_pending(), pending_at: %{}}}
      err -> err
    end
  end

  defp empty_pending, do: %{player: %{}, structure: %{}, depletion: %{}}

  defp upsert_player_row(repo, entry) do
    now = DateTime.utc_now() |> DateTime.truncate(:microsecond)

    attrs = %{
      username: entry.username,
      chunk_x: entry.chunk_x,
      chunk_y: entry.chunk_y,
      x: entry.x,
      y: entry.y,
      inventory: stringify_inventory(entry.inventory),
      inserted_at: now,
      updated_at: now
    }

    repo.insert_all(
      GamePersistence.Schemas.Player,
      [attrs],
      on_conflict: {:replace, [:chunk_x, :chunk_y, :x, :y, :inventory, :updated_at]},
      conflict_target: :username
    )
  end

  defp stringify_inventory(items) do
    for {k, v} <- items, into: %{}, do: {Atom.to_string(k), v}
  end

  defp upsert_structure_row(repo, entry) do
    {chunk_x, chunk_y} = entry.coord
    now = DateTime.utc_now() |> DateTime.truncate(:microsecond)

    attrs = %{
      chunk_x: chunk_x,
      chunk_y: chunk_y,
      owner_username: entry.owner,
      type: Atom.to_string(entry.type),
      x: entry.x,
      y: entry.y,
      hp: entry.hp,
      inserted_at: now,
      updated_at: now
    }

    repo.insert_all(
      GamePersistence.Schemas.Structure,
      [attrs],
      on_conflict: {:replace, [:hp, :updated_at]},
      conflict_target: [:x, :y]
    )
  end

  defp delete_structure_row(repo, x, y) do
    repo.delete_all(
      from(s in GamePersistence.Schemas.Structure, where: s.x == ^x and s.y == ^y)
    )
  end

  defp upsert_depletion_row(repo, {chunk_x, chunk_y}, entry) do
    now = DateTime.utc_now() |> DateTime.truncate(:microsecond)

    attrs = %{
      chunk_x: chunk_x,
      chunk_y: chunk_y,
      type: Atom.to_string(entry.type),
      x: entry.x,
      y: entry.y,
      depleted_until: DateTime.truncate(entry.depleted_until, :microsecond),
      inserted_at: now,
      updated_at: now
    }

    repo.insert_all(
      GamePersistence.Schemas.ResourceNode,
      [attrs],
      on_conflict: {:replace, [:depleted_until, :updated_at]},
      conflict_target: [:chunk_x, :chunk_y, :type, :x, :y]
    )
  end

  defp delete_depletion_row(repo, {chunk_x, chunk_y}, type, x, y) do
    type_str = Atom.to_string(type)

    repo.delete_all(
      from(r in GamePersistence.Schemas.ResourceNode,
        where:
          r.chunk_x == ^chunk_x and r.chunk_y == ^chunk_y and r.type == ^type_str and
            r.x == ^x and r.y == ^y
      )
    )
  end

  defp merged_depletions(state, realm, {chunk_x, chunk_y} = coord) do
    pending_for_coord =
      for {{^realm, ^coord, _type, _x, _y} = key, entry} <- state.pending.depletion, into: %{} do
        {key, entry}
      end

    db_rows =
      from(r in GamePersistence.Schemas.ResourceNode,
        where:
          r.chunk_x == ^chunk_x and r.chunk_y == ^chunk_y and not is_nil(r.depleted_until)
      )
      |> state.repo.all()

    db_view =
      for r <- db_rows, into: %{} do
        type = String.to_existing_atom(r.type)
        key = {realm, coord, type, r.x, r.y}
        {key, %{type: type, x: r.x, y: r.y, depleted_until: r.depleted_until}}
      end

    db_view
    |> Map.merge(pending_for_coord)
    |> Enum.reject(fn {_k, v} -> v == :tombstone end)
    |> Enum.map(fn {_k, v} -> v end)
  end

  defp fetch_player_from_db(repo, username) do
    case repo.get_by(GamePersistence.Schemas.Player, username: username) do
      nil -> nil
      %GamePersistence.Schemas.Player{} = p -> player_to_map(p)
    end
  end

  defp player_to_map(p) do
    %{
      username: p.username,
      chunk_x: p.chunk_x,
      chunk_y: p.chunk_y,
      x: p.x,
      y: p.y,
      inventory: atomize_inventory(p.inventory)
    }
  end

  defp atomize_inventory(nil), do: %{}

  defp atomize_inventory(items) do
    for {k, v} <- items, into: %{}, do: {String.to_existing_atom(k), v}
  end
end
