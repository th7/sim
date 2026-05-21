defmodule GamePersistence.Players do
  @moduledoc "Persistence API for Players. Used on socket connect / chunk flush."

  alias GamePersistence.Repo
  alias GamePersistence.Schemas.Player

  @type inventory :: %{atom() => non_neg_integer()}
  @type position :: %{
          username: String.t(),
          chunk_x: integer(),
          chunk_y: integer(),
          x: integer(),
          y: integer(),
          inventory: inventory()
        }

  @doc """
  Returns the saved state for `username`, creating a row at the default
  origin position if this is the first sight of the Player.
  """
  @spec get_or_create(String.t()) :: position()
  def get_or_create(username) when is_binary(username) do
    case Repo.get_by(Player, username: username) do
      nil ->
        {:ok, player} = %{username: username} |> Player.create_changeset() |> Repo.insert()
        to_position(player)

      player ->
        to_position(player)
    end
  end

  @doc """
  Persists the given position+inventory for `username`. If the row doesn't
  exist yet (it always should, since `get_or_create/1` is called on socket
  connect), this is a no-op.
  """
  @spec upsert_position(
          String.t(),
          {integer(), integer()},
          integer(),
          integer(),
          inventory()
        ) :: :ok
  def upsert_position(username, {chunk_x, chunk_y}, x, y, inventory \\ %{})
      when is_binary(username) do
    case Repo.get_by(Player, username: username) do
      nil ->
        :ok

      %Player{} = player ->
        {:ok, _} =
          player
          |> Player.position_changeset(%{
            chunk_x: chunk_x,
            chunk_y: chunk_y,
            x: x,
            y: y,
            inventory: stringify_inventory(inventory)
          })
          |> Repo.update()

        :ok
    end
  end

  @doc """
  Persist just the Inventory for `username`, leaving position untouched.
  Used by atomic transactions (e.g. build) that mutate inventory without
  a concomitant position update.
  """
  @spec upsert_inventory(String.t(), inventory()) :: :ok
  def upsert_inventory(username, inventory) when is_binary(username) and is_map(inventory) do
    case Repo.get_by(Player, username: username) do
      nil ->
        :ok

      %Player{} = player ->
        {:ok, _} =
          player
          |> Ecto.Changeset.change(%{inventory: stringify_inventory(inventory)})
          |> Repo.update()

        :ok
    end
  end

  defp to_position(%Player{} = p) do
    %{
      username: p.username,
      chunk_x: p.chunk_x,
      chunk_y: p.chunk_y,
      x: p.x,
      y: p.y,
      inventory: atomize_inventory(p.inventory)
    }
  end

  # Inventory crosses the DB boundary as string-keyed JSONB; in memory it's
  # atom-keyed (validated by GameCore.Item.valid?/1). Keys round-trip
  # losslessly because the Item catalogue is a closed enum.
  defp stringify_inventory(items) do
    for {k, v} <- items, into: %{}, do: {Atom.to_string(k), v}
  end

  defp atomize_inventory(nil), do: %{}

  defp atomize_inventory(items) do
    for {k, v} <- items, into: %{}, do: {String.to_existing_atom(k), v}
  end
end
