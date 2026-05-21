defmodule GamePersistence.Repo.Migrations.CreateResourceNodes do
  use Ecto.Migration

  def change do
    create table(:resource_nodes) do
      add :chunk_x, :integer, null: false
      add :chunk_y, :integer, null: false
      add :type, :string, null: false
      add :x, :integer, null: false
      add :y, :integer, null: false
      add :depleted_until, :utc_datetime_usec

      timestamps(type: :utc_datetime_usec)
    end

    create index(:resource_nodes, [:chunk_x, :chunk_y])
  end
end
