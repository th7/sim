defmodule GamePersistence.Schemas.ResourceNode do
  @moduledoc "Persisted gatherable world object (tree, rock, ore vein, plant)."

  use Ecto.Schema
  import Ecto.Changeset

  schema "resource_nodes" do
    field(:chunk_x, :integer)
    field(:chunk_y, :integer)
    field(:type, :string)
    field(:x, :integer)
    field(:y, :integer)
    field(:depleted_until, :utc_datetime_usec)

    timestamps(type: :utc_datetime_usec)
  end

  @required [:chunk_x, :chunk_y, :type, :x, :y]

  def changeset(struct \\ %__MODULE__{}, attrs) do
    struct
    |> cast(attrs, @required ++ [:depleted_until])
    |> validate_required(@required)
  end
end
