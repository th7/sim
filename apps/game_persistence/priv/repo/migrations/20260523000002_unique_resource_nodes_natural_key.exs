defmodule GamePersistence.Repo.Migrations.UniqueResourceNodesNaturalKey do
  use Ecto.Migration

  def change do
    # Natural key for depletion rows. Backs Datastore upserts via
    # ON CONFLICT (chunk_x, chunk_y, type, x, y) DO UPDATE.
    create unique_index(:resource_nodes, [:chunk_x, :chunk_y, :type, :x, :y])
  end
end
