defmodule GameCore.Systems.MovementSystemTest do
  use ExUnit.Case, async: true

  alias GameCore.World
  alias GameCore.Components.{Position, Velocity}
  alias GameCore.Systems.MovementSystem

  test "advances Position by Velocity * dt" do
    world =
      World.new()
      |> World.add_component("alice", Position, %{x: 0.0, y: 0.0})
      |> World.add_component("alice", Velocity, %{vx: 4.0, vy: 0.0})

    world = MovementSystem.run(world, 0.05)

    assert {:ok, pos} = World.fetch(world, "alice", Position)
    assert_in_delta pos.x, 0.2, 1.0e-9
    assert_in_delta pos.y, 0.0, 1.0e-9
  end

  test "entities without a Velocity component do not move" do
    world =
      World.new()
      |> World.add_component("rock", Position, %{x: 5.0, y: 7.0})

    world = MovementSystem.run(world, 0.5)

    assert World.fetch(world, "rock", Position) == {:ok, %{x: 5.0, y: 7.0}}
  end
end
