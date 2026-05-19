defmodule GamePersistence.Repo.Migrations.CreatePlayers do
  use Ecto.Migration

  def change do
    create table(:players) do
      add :username, :string, null: false
      add :chunk_x, :integer, null: false, default: 0
      add :chunk_y, :integer, null: false, default: 0
      add :x, :float, null: false, default: 0.0
      add :y, :float, null: false, default: 0.0

      timestamps(type: :utc_datetime_usec)
    end

    create unique_index(:players, [:username])
    create index(:players, [:chunk_x, :chunk_y])
  end
end
