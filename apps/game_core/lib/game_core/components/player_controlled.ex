defmodule GameCore.Components.PlayerControlled do
  @moduledoc """
  Marker component for entities driven by a human Player. The entity id
  is the Player's username; this component carries no extra data today
  (later phases may add input source, team, etc.).
  """
  @type t :: %{}
end
