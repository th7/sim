defmodule GameCore.Components.Portal do
  @moduledoc """
  An Overworld → Instance entry point, or an Instance → Overworld exit
  point. Worldgen-placed (not player-built), stateless. The `direction`
  field discriminates the two roles: `:into_instance` lives in Overworld
  Chunks; `:out_of_instance` lives in Instance Chunks at a fixed return
  cell.
  """

  defstruct [:type, :direction]

  @type t :: %__MODULE__{
          type: :dungeon,
          direction: :into_instance | :out_of_instance
        }
end
