defmodule GameCore.Systems.BroadcastSystem do
  @moduledoc """
  Produces the wire-format snapshot that subscribers receive on each
  broadcast tick. Snapshot shape is stable across phases:

      %{players: %{username => %{x: float, y: float}}}
  """

  alias GameCore.World
  alias GameCore.Components.{Position, PlayerControlled}

  @type snapshot :: %{players: %{String.t() => GameCore.Components.Position.t()}}

  @spec snapshot(World.t()) :: snapshot()
  def snapshot(%World{components: components}) do
    player_eids = Map.keys(Map.get(components, PlayerControlled, %{}))
    positions = Map.get(components, Position, %{})

    players =
      Enum.reduce(player_eids, %{}, fn eid, acc ->
        case Map.fetch(positions, eid) do
          {:ok, %{x: x, y: y}} -> Map.put(acc, eid, %{x: x, y: y})
          :error -> acc
        end
      end)

    %{players: players}
  end
end
