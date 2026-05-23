defmodule GameCore.Systems.BroadcastSystem do
  @moduledoc """
  Produces the wire-format snapshot that subscribers receive on each
  broadcast tick. Shape:

      %{
        players: %{username => %{x: int, y: int}},
        resource_nodes: %{wire_id => %{type, x, y, depleted}},
        structures: %{wire_id => %{type, x, y, hp, owner}}
      }

  Positions are in sub-units; the frontend divides by 1000 at the channel
  boundary to render in world units.
  """

  alias GameCore.World

  alias GameCore.Components.{
    Depleted,
    Gatherable,
    PlayerControlled,
    Portal,
    Position,
    Structure
  }

  @type snapshot :: %{
          players: %{String.t() => %{x: integer(), y: integer()}},
          resource_nodes: %{String.t() => map()},
          structures: %{String.t() => map()},
          portals: %{String.t() => map()}
        }

  @spec snapshot(World.t()) :: snapshot()
  def snapshot(%World{} = world) do
    %{
      players: players(world),
      resource_nodes: resource_nodes(world),
      structures: structures(world),
      portals: portals(world)
    }
  end

  defp portals(%World{components: components}) do
    positions = Map.get(components, Position, %{})
    portals = Map.get(components, Portal, %{})

    Enum.reduce(portals, %{}, fn {eid, %{type: type, direction: dir}}, acc ->
      case Map.fetch(positions, eid) do
        {:ok, %{x: x, y: y}} ->
          Map.put(acc, eid, %{
            type: Atom.to_string(type),
            direction: Atom.to_string(dir),
            x: x,
            y: y
          })

        :error ->
          acc
      end
    end)
  end

  defp players(%World{components: components}) do
    player_eids = Map.keys(Map.get(components, PlayerControlled, %{}))
    positions = Map.get(components, Position, %{})

    Enum.reduce(player_eids, %{}, fn eid, acc ->
      case Map.fetch(positions, eid) do
        {:ok, %{x: x, y: y}} -> Map.put(acc, eid, %{x: x, y: y})
        :error -> acc
      end
    end)
  end

  defp resource_nodes(%World{components: components}) do
    positions = Map.get(components, Position, %{})
    gatherables = Map.get(components, Gatherable, %{})
    depleteds = Map.get(components, Depleted, %{})

    node_eids =
      MapSet.union(MapSet.new(Map.keys(gatherables)), MapSet.new(Map.keys(depleteds)))

    Enum.reduce(node_eids, %{}, fn eid, acc ->
      case Map.fetch(positions, eid) do
        {:ok, %{x: x, y: y}} ->
          type =
            case Map.get(gatherables, eid) || Map.get(depleteds, eid) do
              %{type: t} -> Atom.to_string(t)
            end

          Map.put(acc, eid, %{
            type: type,
            x: x,
            y: y,
            depleted: Map.has_key?(depleteds, eid)
          })

        :error ->
          acc
      end
    end)
  end

  defp structures(%World{components: components}) do
    positions = Map.get(components, Position, %{})
    structs = Map.get(components, Structure, %{})

    Enum.reduce(structs, %{}, fn {eid, %{type: type, owner: owner, hp: hp}}, acc ->
      case Map.fetch(positions, eid) do
        {:ok, %{x: x, y: y}} ->
          Map.put(acc, eid, %{
            type: Atom.to_string(type),
            x: x,
            y: y,
            hp: hp,
            owner: owner
          })

        :error ->
          acc
      end
    end)
  end
end
