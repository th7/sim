defmodule GameCore.Systems.BroadcastSystemTest do
  use ExUnit.Case, async: true

  alias GameCore.World
  alias GameCore.Components.{Position, PlayerControlled}
  alias GameCore.Systems.BroadcastSystem

  test "snapshot maps PlayerControlled entities to %{players: %{username => %{x, y}}}" do
    world =
      World.new()
      |> World.add_component("alice", Position, %{x: 1.0, y: 2.0})
      |> World.add_component("alice", PlayerControlled, %{})
      |> World.add_component("bob", Position, %{x: -3.0, y: 4.0})
      |> World.add_component("bob", PlayerControlled, %{})

    assert BroadcastSystem.snapshot(world) == %{
             players: %{
               "alice" => %{x: 1.0, y: 2.0},
               "bob" => %{x: -3.0, y: 4.0}
             }
           }
  end

  test "entities without PlayerControlled are not in the snapshot" do
    world =
      World.new()
      |> World.add_component("alice", Position, %{x: 1.0, y: 2.0})
      |> World.add_component("alice", PlayerControlled, %{})
      |> World.add_component(42, Position, %{x: 9.0, y: 9.0})

    assert BroadcastSystem.snapshot(world) == %{
             players: %{"alice" => %{x: 1.0, y: 2.0}}
           }
  end
end
