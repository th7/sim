defmodule GamePersistence.Repo.Migrations.IndexResourceNodesDepletedUntil do
  use Ecto.Migration

  def change do
    # DepletionPruner sweeps `WHERE depleted_until < now()` periodically;
    # this partial index makes that DELETE cheap as the table grows.
    create index(:resource_nodes, [:depleted_until],
             where: "depleted_until IS NOT NULL"
           )
  end
end
