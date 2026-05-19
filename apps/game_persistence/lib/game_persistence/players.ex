defmodule GamePersistence.Players do
  @moduledoc "Persistence API for Players. Used on socket connect / chunk flush."

  alias GamePersistence.Repo
  alias GamePersistence.Schemas.Player

  @type position :: %{
          username: String.t(),
          chunk_x: integer(),
          chunk_y: integer(),
          x: float(),
          y: float()
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
  Persists the given position for `username`. If the row doesn't exist yet
  (it always should, since `get_or_create/1` is called on socket connect),
  this is a no-op.
  """
  @spec upsert_position(String.t(), {integer(), integer()}, float(), float()) :: :ok
  def upsert_position(username, {chunk_x, chunk_y}, x, y) when is_binary(username) do
    case Repo.get_by(Player, username: username) do
      nil ->
        :ok

      %Player{} = player ->
        {:ok, _} =
          player
          |> Player.position_changeset(%{chunk_x: chunk_x, chunk_y: chunk_y, x: x, y: y})
          |> Repo.update()

        :ok
    end
  end

  defp to_position(%Player{} = p) do
    %{username: p.username, chunk_x: p.chunk_x, chunk_y: p.chunk_y, x: p.x, y: p.y}
  end
end
