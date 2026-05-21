defmodule GameCore.WorldTest do
  use ExUnit.Case, async: true

  alias GameCore.World
  alias GameCore.Components.Position

  test "components round-trip through add/fetch/remove_entity" do
    world =
      World.new()
      |> World.add_component("alice", Position, %{x: 1_000, y: 2_000})

    assert World.fetch(world, "alice", Position) == {:ok, %{x: 1_000, y: 2_000}}

    world = World.remove_entity(world, "alice")
    assert World.fetch(world, "alice", Position) == :error
  end
end
