defmodule GamePersistence.Repo.Migrations.UniqueStructuresXy do
  use Ecto.Migration

  def change do
    create unique_index(:structures, [:x, :y])
  end
end
