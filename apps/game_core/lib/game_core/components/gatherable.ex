defmodule GameCore.Components.Gatherable do
  @moduledoc """
  Marker (with payload) for a Resource node that is currently harvestable.
  Mutually exclusive with `GameCore.Components.Depleted` — a node has
  exactly one of the two at any time.
  """
  @type t :: %{type: atom(), yields: GameCore.Item.t()}
end
