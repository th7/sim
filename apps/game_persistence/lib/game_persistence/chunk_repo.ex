defmodule GamePersistence.ChunkRepo do
  @moduledoc "Postgres-backed implementation of `GameCore.ChunkRepo`."

  @behaviour GameCore.ChunkRepo

  alias GamePersistence.{Players, Repo}
  alias GamePersistence.Schemas.{ResourceNode, Structure}

  @impl true
  def fetch_player(username) when is_binary(username), do: Players.get_or_create(username)

  @impl true
  def flush_players(coord, players) when is_list(players) do
    Enum.each(players, fn p ->
      Players.upsert_position(p.username, coord, p.x, p.y, Map.get(p, :inventory, %{}))
    end)

    :ok
  end

  @impl true
  def build_structure({chunk_x, chunk_y}, owner, type, x, y, new_inventory)
      when is_atom(type) and is_binary(owner) and is_integer(x) and is_integer(y) do
    attrs = %{
      chunk_x: chunk_x,
      chunk_y: chunk_y,
      owner_username: owner,
      type: Atom.to_string(type),
      x: x,
      y: y,
      hp: GameCore.Structure.Catalogue.max_hp(type)
    }

    Repo.transaction(fn ->
      case Structure.changeset(attrs) |> Repo.insert() do
        {:ok, s} ->
          :ok = Players.upsert_inventory(owner, new_inventory)
          s.id

        {:error, _} = err ->
          Repo.rollback(err)
      end
    end)
    |> case do
      {:ok, id} -> {:ok, id}
      {:error, _} -> {:error, :build_failed}
    end
  end

  @impl true
  def destroy_structure(id) when is_integer(id) do
    case Repo.get(Structure, id) do
      nil -> :ok
      %Structure{} = s -> Repo.delete!(s) && :ok
    end
  end

  @impl true
  def fetch_structures({chunk_x, chunk_y}) do
    import Ecto.Query

    Repo.all(from(s in Structure, where: s.chunk_x == ^chunk_x and s.chunk_y == ^chunk_y))
    |> Enum.map(fn s ->
      %{
        id: s.id,
        type: String.to_existing_atom(s.type),
        owner: s.owner_username,
        x: s.x,
        y: s.y,
        hp: s.hp
      }
    end)
  end

  @impl true
  def fetch_depletions({chunk_x, chunk_y}) do
    import Ecto.Query

    Repo.all(
      from(r in ResourceNode,
        where: r.chunk_x == ^chunk_x and r.chunk_y == ^chunk_y and not is_nil(r.depleted_until)
      )
    )
    |> Enum.map(fn r ->
      %{
        type: String.to_existing_atom(r.type),
        x: r.x,
        y: r.y,
        depleted_until: r.depleted_until
      }
    end)
  end

  @impl true
  def flush_depletions({chunk_x, chunk_y}, depleted_now) when is_list(depleted_now) do
    import Ecto.Query

    # The chunk's GenServer serializes all writes to its own rows, so the
    # cheapest correct reconcile is DELETE-all + INSERT-all inside one
    # transaction. The set is tiny (a handful of trees per chunk).
    now = DateTime.utc_now() |> DateTime.truncate(:microsecond)

    rows =
      Enum.map(depleted_now, fn d ->
        %{
          chunk_x: chunk_x,
          chunk_y: chunk_y,
          type: Atom.to_string(d.type),
          x: d.x,
          y: d.y,
          depleted_until: DateTime.truncate(d.depleted_until, :microsecond),
          inserted_at: now,
          updated_at: now
        }
      end)

    Repo.transaction(fn ->
      Repo.delete_all(
        from(r in ResourceNode, where: r.chunk_x == ^chunk_x and r.chunk_y == ^chunk_y)
      )

      if rows != [], do: Repo.insert_all(ResourceNode, rows)
    end)

    :ok
  end
end
