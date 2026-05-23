defmodule GameCore.Systems.MovementSystemTest do
  use ExUnit.Case, async: true

  alias GameCore.World
  alias GameCore.Components.{Position, Velocity}
  alias GameCore.Systems.MovementSystem

  test "advances Position by Velocity * dt, rounding to integer sub-units" do
    world =
      World.new()
      |> World.add_component("alice", Position, %{x: 0, y: 0})
      |> World.add_component("alice", Velocity, %{vx: 4_000.0, vy: 0.0})

    world = MovementSystem.run(world, 0.05)

    # 4000 sub-units/sec * 0.05s = 200 sub-units (= 0.2 world units).
    assert {:ok, %{x: 200, y: 0}} = World.fetch(world, "alice", Position)
  end

  test "entities without a Velocity component do not move" do
    world =
      World.new()
      |> World.add_component("rock", Position, %{x: 5_000, y: 7_000})

    world = MovementSystem.run(world, 0.5)

    assert World.fetch(world, "rock", Position) == {:ok, %{x: 5_000, y: 7_000}}
  end

  test "with bounds, clamps Position to the bounding rect on each tick" do
    world =
      World.new()
      |> World.add_component("alice", Position, %{x: 47_000, y: 8_000})
      |> World.add_component("alice", Velocity, %{vx: 10_000.0, vy: 0.0})

    # Bounds = 3x3 Instance grid: cx 0..2, cy 0..2 → x ∈ [0, 48_000], y ∈ [0, 48_000].
    world = MovementSystem.run(world, 1.0, bounds: {0, 0, 48_000, 48_000})

    assert {:ok, %{x: 48_000, y: 8_000}} = World.fetch(world, "alice", Position)
  end

  test "without bounds, does not clamp" do
    world =
      World.new()
      |> World.add_component("alice", Position, %{x: 47_000, y: 0})
      |> World.add_component("alice", Velocity, %{vx: 10_000.0, vy: 0.0})

    world = MovementSystem.run(world, 1.0)

    assert {:ok, %{x: 57_000, y: 0}} = World.fetch(world, "alice", Position)
  end
end
