defmodule GamePersistence.Repo.Migrations.CreateStructures do
  use Ecto.Migration

  def change do
    create table(:structures) do
      add :chunk_x, :integer, null: false
      add :chunk_y, :integer, null: false
      add :owner_username, :string, null: false
      add :type, :string, null: false
      add :x, :float, null: false
      add :y, :float, null: false
      add :hp, :integer, null: false, default: 100

      timestamps(type: :utc_datetime_usec)
    end

    create index(:structures, [:chunk_x, :chunk_y])
    create index(:structures, [:owner_username])
  end
end
