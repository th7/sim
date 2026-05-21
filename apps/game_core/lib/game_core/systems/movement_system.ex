defmodule GameCore.Systems.MovementSystem do
  @moduledoc """
  Integrates Velocity into Position for every entity that has both.
  Position is integer sub-units; Velocity is float sub-units/sec; the
  integrated step is rounded back to integer at each tick.
  """

  alias GameCore.World
  alias GameCore.Components.{Position, Velocity}

  @spec run(World.t(), float()) :: World.t()
  def run(%World{components: components} = world, dt) when is_number(dt) do
    velocities = Map.get(components, Velocity, %{})

    Enum.reduce(velocities, world, fn {eid, %{vx: vx, vy: vy}}, acc ->
      case World.fetch(acc, eid, Position) do
        {:ok, %{x: x, y: y}} ->
          World.add_component(acc, eid, Position, %{
            x: x + round(vx * dt),
            y: y + round(vy * dt)
          })

        :error ->
          acc
      end
    end)
  end
end
