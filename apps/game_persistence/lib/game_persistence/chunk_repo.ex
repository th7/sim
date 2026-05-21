defmodule GamePersistence.ChunkRepo do
  @moduledoc "Postgres-backed implementation of `GameCore.ChunkRepo`."

  @behaviour GameCore.ChunkRepo

  alias GamePersistence.{Players, Repo}
  alias GamePersistence.Schemas.Structure

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

    Repo.all(from s in Structure, where: s.chunk_x == ^chunk_x and s.chunk_y == ^chunk_y)
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
end
