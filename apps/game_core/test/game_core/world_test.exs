defmodule GameCore.WorldTest do
  use ExUnit.Case, async: true

  alias GameCore.World
  alias GameCore.Components.Position

  test "components round-trip through add/fetch/remove_entity" do
    world =
      World.new()
      |> World.add_component("alice", Position, %{x: 1.0, y: 2.0})

    assert World.fetch(world, "alice", Position) == {:ok, %{x: 1.0, y: 2.0}}

    world = World.remove_entity(world, "alice")
    assert World.fetch(world, "alice", Position) == :error
  end
end
