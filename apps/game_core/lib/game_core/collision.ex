defmodule GameCore.Collision do
  @moduledoc """
  Movement collision: axis-decomposed clamping of a body circle's step
  against the **Footprint**s of obstacle entities in the World; and the
  symmetric build-time predicate (`aabb_blocked?`) used to reject a
  proposed Footprint that would overlap any existing one — or any
  Player's body.

  Per-axis stop gives free slide along axis-aligned walls. A body that
  is currently overlapping any Footprint is grandfathered: it moves
  freely until clear, then normal collision applies. This keeps
  spawn-on-obstacle cases from becoming permanently stuck.
  """

  alias GameCore.World
  alias GameCore.Components.{Footprint, PlayerControlled, Position}

  @player_body_radius 300

  @type point :: {integer(), integer()}
  @type step :: {integer(), integer()}

  @spec clamp_step(World.t(), point(), step()) :: point()
  def clamp_step(%World{} = world, {cx, cy}, {dx, dy}) do
    obstacles = collect_obstacles(world)
    r = @player_body_radius

    if Enum.any?(obstacles, &overlaps?(&1, cx, cy, r)) do
      {cx + dx, cy + dy}
    else
      clamped_dx = Enum.reduce(obstacles, dx, &limit_axis(&1, cx, cy, r, &2, :x))
      new_x = cx + clamped_dx

      clamped_dy = Enum.reduce(obstacles, dy, &limit_axis(&1, new_x, cy, r, &2, :y))
      new_y = cy + clamped_dy

      {new_x, new_y}
    end
  end

  @spec aabb_blocked?(World.t(), point(), %{w: pos_integer(), h: pos_integer()}) :: boolean()
  def aabb_blocked?(%World{} = world, {x, y}, %{w: w, h: h}) do
    aabb_vs_footprints?(world, x, y, w, h) or aabb_vs_players?(world, x, y, w, h)
  end

  defp collect_obstacles(%World{components: components}) do
    positions = Map.get(components, Position, %{})
    footprints = Map.get(components, Footprint, %{})

    Enum.reduce(footprints, [], fn {eid, fp}, acc ->
      case Map.fetch(positions, eid) do
        {:ok, %{x: x, y: y}} -> [{x, y, fp} | acc]
        :error -> acc
      end
    end)
  end

  defp aabb_vs_footprints?(%World{components: components}, x, y, w, h) do
    positions = Map.get(components, Position, %{})
    footprints = Map.get(components, Footprint, %{})

    Enum.any?(footprints, fn {eid, fp} ->
      case Map.fetch(positions, eid) do
        {:ok, %{x: ox, y: oy}} -> aabb_fp_overlap?(x, y, w, h, ox, oy, fp)
        :error -> false
      end
    end)
  end

  defp aabb_vs_players?(%World{components: components}, x, y, w, h) do
    positions = Map.get(components, Position, %{})
    pcs = Map.get(components, PlayerControlled, %{})

    Enum.any?(pcs, fn {eid, _} ->
      case Map.fetch(positions, eid) do
        {:ok, %{x: px, y: py}} -> aabb_circle_overlap?(x, y, w, h, px, py, @player_body_radius)
        :error -> false
      end
    end)
  end

  defp aabb_fp_overlap?(x, y, w, h, ox, oy, %{shape: :aabb, w: ow, h: oh}) do
    abs(x - ox) * 2 < w + ow and abs(y - oy) * 2 < h + oh
  end

  defp aabb_fp_overlap?(x, y, w, h, ox, oy, %{shape: :circle, radius: orad}) do
    aabb_circle_overlap?(x, y, w, h, ox, oy, orad)
  end

  defp aabb_circle_overlap?(x, y, w, h, cx, cy, r) do
    half_w = div(w, 2)
    half_h = div(h, 2)
    nearest_x = cx |> max(x - half_w) |> min(x + half_w)
    nearest_y = cy |> max(y - half_h) |> min(y + half_h)
    ddx = cx - nearest_x
    ddy = cy - nearest_y
    ddx * ddx + ddy * ddy < r * r
  end

  defp overlaps?({ox, oy, %{shape: :aabb, w: w, h: h}}, cx, cy, r) do
    aabb_circle_overlap?(ox, oy, w, h, cx, cy, r)
  end

  defp overlaps?({ox, oy, %{shape: :circle, radius: orad}}, cx, cy, r) do
    ddx = cx - ox
    ddy = cy - oy
    rsum = r + orad
    ddx * ddx + ddy * ddy < rsum * rsum
  end

  defp limit_axis({ox, oy, %{shape: :aabb, w: w, h: h}}, cx, cy, r, step, axis) do
    half_w = div(w, 2)
    half_h = div(h, 2)
    ax_min = ox - half_w
    ax_max = ox + half_w
    ay_min = oy - half_h
    ay_max = oy + half_h

    case axis do
      :x ->
        if cy + r > ay_min and cy - r < ay_max do
          limit_linear(step, cx, ax_min, ax_max, r)
        else
          step
        end

      :y ->
        if cx + r > ax_min and cx - r < ax_max do
          limit_linear(step, cy, ay_min, ay_max, r)
        else
          step
        end
    end
  end

  defp limit_axis({ox, oy, %{shape: :circle, radius: orad}}, cx, cy, r, step, axis) do
    rsum = r + orad
    rsum2 = rsum * rsum

    {center, obstacle_center, perp2} =
      case axis do
        :x -> {cx, ox, (cy - oy) * (cy - oy)}
        :y -> {cy, oy, (cx - ox) * (cx - ox)}
      end

    if perp2 < rsum2 do
      sqrt_term = :math.sqrt(rsum2 - perp2)
      root1 = obstacle_center - center - sqrt_term
      root2 = obstacle_center - center + sqrt_term

      cond do
        step > 0 and root1 >= 0 ->
          min(step, trunc(:math.floor(root1)))

        step < 0 and root2 <= 0 ->
          max(step, trunc(:math.ceil(root2)))

        true ->
          step
      end
    else
      step
    end
  end

  defp limit_linear(step, center, lo, hi, r) do
    cond do
      step > 0 and lo > center -> min(step, lo - r - center)
      step < 0 and hi < center -> max(step, hi + r - center)
      true -> step
    end
  end
end
