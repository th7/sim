defmodule GameCore do
  @moduledoc """
  Public entry points for the pure game core: starting Chunks under the
  shared `DynamicSupervisor` and registering them in the `Chunks` registry.
  """

  @doc """
  Start a Chunk under `GameCore.ChunkSupervisor`, registered by coord in
  `GameCore.Chunks`. Accepts the same options as `GameCore.Chunk.start_link/1`
  (the `:name` option is filled in automatically from `:coord`).
  """
  def start_chunk(opts) do
    coord = Keyword.fetch!(opts, :coord)
    opts = Keyword.put_new(opts, :name, GameCore.Chunks.via(coord))
    DynamicSupervisor.start_child(GameCore.ChunkSupervisor, {GameCore.Chunk, opts})
  end
end
