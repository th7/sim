defmodule GameCore.Systems.MovementSystem do
  @moduledoc """
  Integrates Velocity into Position for every entity that has both.
  Position is integer sub-units; Velocity is float sub-units/sec; the
  integrated step is rounded back to integer at each tick.

  When given a `:bounds` option `{x_min, y_min, x_max, y_max}`, the
  integrated Position is clamped to the bounding rect. The Overworld
  runs without bounds (unbounded); each Instance Chunk runs with its
  3×3 grid's outer rect.
  """

  alias GameCore.World
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
          new_x = x + round(vx * dt)
          new_y = y + round(vy * dt)
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
