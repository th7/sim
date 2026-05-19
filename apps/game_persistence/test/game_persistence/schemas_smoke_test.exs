defmodule GamePersistence.SchemasSmokeTest do
  use GamePersistence.DataCase, async: true

  alias GamePersistence.Schemas.{Structure, ResourceNode}

  test "Structure round-trips" do
    {:ok, s} =
      Structure.changeset(%{
        chunk_x: 0,
        chunk_y: 0,
        owner_username: "alice",
        type: "wall",
        x: 1.0,
        y: 2.0
      })
      |> Repo.insert()

    assert s.hp == 100
    assert Repo.get(Structure, s.id).type == "wall"
  end

  test "ResourceNode round-trips with a null depleted_until" do
    {:ok, r} =
      ResourceNode.changeset(%{
        chunk_x: 1,
        chunk_y: -1,
        type: "tree",
        x: 5.0,
        y: 6.0
      })
      |> Repo.insert()

    assert is_nil(r.depleted_until)
    assert Repo.get(ResourceNode, r.id).type == "tree"
  end
end
