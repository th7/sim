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
  @type inventory :: %{atom() => non_neg_integer()}
  @type position :: %{
          username: username(),
          chunk_x: integer(),
          chunk_y: integer(),
          x: integer(),
          y: integer(),
          inventory: inventory()
        }

  @doc "Fetch the last-saved position+inventory for `username`, or `nil` if unknown."
  @callback fetch_player(username()) :: position() | nil

  @doc "Persist the given player positions+inventories, tagging them all with `coord`."
  @callback flush_players(coord(), [
              %{
                username: username(),
                x: integer(),
                y: integer(),
                inventory: inventory()
              }
            ]) :: :ok

  @doc """
  Atomic build: INSERT a new Structure row + UPDATE the placing Player's
  Inventory in one transaction. Returns `{:ok, structure_id}` on commit;
  `{:error, reason}` otherwise (leaves no state change).
  """
  @callback build_structure(
              coord(),
              owner :: username(),
              type :: atom(),
              x :: integer(),
              y :: integer(),
              inventory()
            ) :: {:ok, integer()} | {:error, atom()}

  @doc "DELETE the persisted Structure row with the given id."
  @callback destroy_structure(id :: integer()) :: :ok

  @type structure_row :: %{
          id: integer(),
          type: atom(),
          owner: username(),
          x: integer(),
          y: integer(),
          hp: non_neg_integer()
        }

  @doc "List all persisted Structures for the given chunk coord."
  @callback fetch_structures(coord()) :: [structure_row()]
end
