defmodule GameCore.Components.Depleted do
  @moduledoc """
  Marker (with payload) for a Resource node that has been harvested and
  is awaiting respawn. `depleted_until` is a wall-clock `DateTime` so it
  round-trips through the `resource_nodes` table; the Chunk schedules
  a `Process.send_after({:respawn, eid}, remaining_ms)` whose timer
  fires near that instant. Mutually exclusive with
  `GameCore.Components.Gatherable`.
  """
  @type t :: %{type: atom(), depleted_until: DateTime.t()}
end
