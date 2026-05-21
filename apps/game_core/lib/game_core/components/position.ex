defmodule GameCore.Components.Position do
  @moduledoc """
  World-space x/y of an entity, in **sub-units**. 1 world unit = 1000
  sub-units; conversions to world-unit floats happen at the channel
  boundary on their way to the frontend. Integer arithmetic everywhere
  in the server preserves exact identity for interaction checks and
  for DB row lookups against Worldgen-derived coordinates.
  """
  @type t :: %{x: integer(), y: integer()}
end
