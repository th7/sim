defmodule GameCore.Components.Renderable do
  @moduledoc """
  Marker that an entity should be included in client-visible snapshots.
  Today the field is unused; future phases may carry mesh kind, color, etc.
  """
  @type t :: %{}
end
