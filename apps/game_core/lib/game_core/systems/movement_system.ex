defmodule GameCore.Systems.MovementSystem do
  @moduledoc """
  Integrates Velocity into Position for every entity that has both.
  Position is integer sub-units; Velocity is float sub-units/sec; the
  integrated step is rounded back to integer at each tick.

  Each integrated step is clamped against any **Footprint** in the world
  via `GameCore.Collision`, then clamped to the optional bounding rect
  (Instance Chunks use this; the Overworld runs unbounded).
  """

  alias GameCore.{Collision, World}
  alias GameCore.Components.{Position, Velocity}

  @type bounds :: {integer(), integer(), integer(), integer()}

  @spec run(World.t(), float(), keyword()) :: World.t()
  def run(world, dt, opts \\ [])

  def run(%World{components: components} = world, dt, opts) when is_number(dt) do
    velocities = Map.get(components, Velocity, %{})
    bounds = Keyword.get(opts, :bounds)

    Enum.reduce(velocities, world, fn {eid, %{vx: vx, vy: vy}}, acc ->
      case World.fetch(acc, eid, Position) do
        {:ok, %{x: x, y: y}} ->
          step = {round(vx * dt), round(vy * dt)}
          {new_x, new_y} = Collision.clamp_step(acc, {x, y}, step)
          {clamped_x, clamped_y} = clamp(new_x, new_y, bounds)

          World.add_component(acc, eid, Position, %{x: clamped_x, y: clamped_y})

        :error ->
          acc
      end
    end)
  end

  defp clamp(x, y, nil), do: {x, y}

  defp clamp(x, y, {x_min, y_min, x_max, y_max}) do
    {x |> max(x_min) |> min(x_max), y |> max(y_min) |> min(y_max)}
  end
end
