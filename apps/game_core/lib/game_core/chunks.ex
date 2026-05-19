defmodule GameCore.Chunks do
  @moduledoc """
  Lookup and naming for live Chunk processes, keyed by coord.
  """

  @registry __MODULE__

  @doc "A `:via` name for registering a Chunk under its coord."
  @spec via(GameCore.Chunk.coord()) :: {:via, Registry, {module(), GameCore.Chunk.coord()}}
  def via(coord), do: {:via, Registry, {@registry, coord}}

  @doc "Look up the live Chunk process for a coord, or `nil` if cold."
  @spec whereis(GameCore.Chunk.coord()) :: pid() | nil
  def whereis(coord) do
    case Registry.lookup(@registry, coord) do
      [{pid, _}] -> pid
      [] -> nil
    end
  end
end
