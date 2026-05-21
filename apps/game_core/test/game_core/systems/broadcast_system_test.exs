defmodule GameCore.Systems.BroadcastSystemTest do
  use ExUnit.Case, async: true

  alias GameCore.World
  alias GameCore.Components.{Position, PlayerControlled}
  alias GameCore.Systems.BroadcastSystem

  test "snapshot maps PlayerControlled entities to %{players: %{username => %{x, y}}}" do
    world =
      World.new()
      |> World.add_component("alice", Position, %{x: 1_000, y: 2_000})
      |> World.add_component("alice", PlayerControlled, %{})
      |> World.add_component("bob", Position, %{x: -3_000, y: 4_000})
      |> World.add_component("bob", PlayerControlled, %{})

    snap = BroadcastSystem.snapshot(world)

    assert snap.players == %{
             "alice" => %{x: 1_000, y: 2_000},
             "bob" => %{x: -3_000, y: 4_000}
           }

    assert snap.resource_nodes == %{}
  end

  test "entities without PlayerControlled are not in the snapshot" do
    world =
      World.new()
      |> World.add_component("alice", Position, %{x: 1_000, y: 2_000})
      |> World.add_component("alice", PlayerControlled, %{})
      |> World.add_component(42, Position, %{x: 9_000, y: 9_000})

    snap = BroadcastSystem.snapshot(world)
    assert snap.players == %{"alice" => %{x: 1_000, y: 2_000}}
    assert snap.resource_nodes == %{}
  end
end
