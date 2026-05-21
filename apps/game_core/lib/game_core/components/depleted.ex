defmodule GameCore.Components.Depleted do
  @moduledoc """
  Marker (with payload) for a Resource node that has been harvested and
  is awaiting respawn. `depleted_until` is a `System.monotonic_time(:millisecond)`
  value; `nil` means "depleted indefinitely" (used transiently before a
  timer is scheduled). Mutually exclusive with `GameCore.Components.Gatherable`.
  """
  @type t :: %{type: atom(), depleted_until: integer() | nil}
end
