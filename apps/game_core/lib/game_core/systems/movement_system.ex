defmodule GameCore.Systems.MovementSystem do
  @moduledoc "Integrates Velocity into Position for every entity that has both."

  alias GameCore.World
  alias GameCore.Components.{Position, Velocity}

  @spec run(World.t(), float()) :: World.t()
  def run(%World{components: components} = world, dt) when is_number(dt) do
    velocities = Map.get(components, Velocity, %{})

    Enum.reduce(velocities, world, fn {eid, %{vx: vx, vy: vy}}, acc ->
      case World.fetch(acc, eid, Position) do
        {:ok, %{x: x, y: y}} ->
          World.add_component(acc, eid, Position, %{x: x + vx * dt, y: y + vy * dt})

        :error ->
          acc
      end
    end)
  end
end
