defmodule GamePersistence.Schemas.Player do
  @moduledoc """
  Persisted state of a Player between sessions. A row is created on first
  sight in `UserSocket.connect`; position is flushed on disconnect and
  periodically while connected.
  """

  use Ecto.Schema
  import Ecto.Changeset

  schema "players" do
    field(:username, :string)
    field(:chunk_x, :integer, default: 0)
    field(:chunk_y, :integer, default: 0)
    # New Players spawn at the centre of chunk (0,0) — 8000 sub-units in
    # each axis (half of `GameCore.ChunkGeometry.chunk_size/0`). This puts
    # them within harvest range of the Worldgen trees clustered around
    # chunk-centre, rather than at the corner where they'd have to walk
    # to find their first tree.
    field(:x, :integer, default: 8_000)
    field(:y, :integer, default: 8_000)
    field(:inventory, :map, default: %{})

    timestamps(type: :utc_datetime_usec)
  end

  @required_for_create [:username]
  @position_fields [:chunk_x, :chunk_y, :x, :y, :inventory]

  def create_changeset(attrs) do
    %__MODULE__{}
    |> cast(attrs, @required_for_create ++ @position_fields)
    |> validate_required(@required_for_create)
    |> unique_constraint(:username)
  end

  def position_changeset(player, attrs) do
    player
    |> cast(attrs, @position_fields)
    |> validate_required(@position_fields)
  end
end
