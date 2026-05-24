defmodule GameCore.Components.Footprint do
  @moduledoc """
  The world-space shape an obstacle occupies for movement collision.
  Combined with the entity's `Position`, a Footprint defines the area
  a Player's body cannot enter: a circle (radius centered at Position)
  or an axis-aligned rectangle (full width × full height centered at
  Position). Players carry no Footprint — collision is one-way.
  """

  @type t ::
          %{shape: :circle, radius: pos_integer()}
          | %{shape: :aabb, w: pos_integer(), h: pos_integer()}
end
