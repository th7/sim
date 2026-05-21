defmodule GameCore.Components.Structure do
  @moduledoc """
  Player-placed Structure (wall, etc) anchored to a Chunk. The ECS entity
  id mirrors the persisted `structures.id` as a string, so the wire snapshot
  carries a stable identifier across chunk reactivations.
  """
  @type t :: %{
          type: atom(),
          owner: String.t(),
          hp: non_neg_integer()
        }
end
