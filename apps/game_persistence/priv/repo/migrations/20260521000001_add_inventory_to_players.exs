defmodule GamePersistence.Repo.Migrations.AddInventoryToPlayers do
  use Ecto.Migration

  def change do
    alter table(:players) do
      # `:map` -> jsonb; literal map defaults need Jason at migration time
      # (not loaded here), so use a fragment.
      add :inventory, :map, null: false, default: fragment("'{}'::jsonb")
    end
  end
end
