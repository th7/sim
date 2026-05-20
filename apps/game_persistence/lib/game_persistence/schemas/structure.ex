defmodule GamePersistence.Schemas.Structure do
  @moduledoc "Persisted Structure (building/wall/etc) anchored to a Chunk."

  use Ecto.Schema
  import Ecto.Changeset

  schema "structures" do
    field(:chunk_x, :integer)
    field(:chunk_y, :integer)
    field(:owner_username, :string)
    field(:type, :string)
    field(:x, :float)
    field(:y, :float)
    field(:hp, :integer, default: 100)

    timestamps(type: :utc_datetime_usec)
  end

  @required [:chunk_x, :chunk_y, :owner_username, :type, :x, :y]

  def changeset(struct \\ %__MODULE__{}, attrs) do
    struct
    |> cast(attrs, @required ++ [:hp])
    |> validate_required(@required)
  end
end
