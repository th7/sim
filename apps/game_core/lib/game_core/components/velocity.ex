defmodule GameCore.Components.Velocity do
  @moduledoc """
  Per-second velocity in sub-units/second. Floats are tolerated here
  because Velocity is recomputed from intent every `set_intent` call —
  it is never accumulated, so FP precision does not drift Position
  (Position is integer; MovementSystem rounds the integrated step).
  """
  @type t :: %{vx: float(), vy: float()}
end
