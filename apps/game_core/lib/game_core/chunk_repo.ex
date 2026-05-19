defmodule GameCore.ChunkRepo do
  @moduledoc """
  Persistence contract a Chunk uses to hydrate Player state on join and
  flush it on leave / on a periodic tick. Implemented by `game_persistence`;
  `GameCore.ChunkRepo.Null` is the default for tests that don't care about
  durability.

  Keeping this as a behaviour (rather than `game_core` depending on
  `game_persistence`) preserves the umbrella boundary: `game_core` is pure
  Elixir with no Ecto dependency.
  """

  @type coord :: {integer(), integer()}
  @type username :: String.t()
  @type position :: %{
          username: username(),
          chunk_x: integer(),
          chunk_y: integer(),
          x: float(),
          y: float()
        }

  @doc "Fetch the last-saved position for `username`, or `nil` if unknown."
  @callback fetch_player(username()) :: position() | nil

  @doc "Persist the given player positions, tagging them all with `coord`."
  @callback flush_players(coord(), [%{username: username(), x: float(), y: float()}]) ::
              :ok
end
