defmodule GameCore.Worldgen do
  @moduledoc """
  Compile-time, deterministic placement of Resource nodes per Chunk.
  Pure function `resource_nodes/1` — given a chunk coord, returns the
  list of `{type, x, y}` entries that define the chunk's content. The
  DB only stores depletion state, never positions.

  v1 places a handful of Trees in a fixed pattern around each chunk's
  center. Procedural density / multi-type generation arrive in a later
  phase.
  """

  alias GameCore.ChunkGeometry

  @type node_spec :: %{type: :tree, x: integer(), y: integer()}

  # Tree offsets from chunk-center, in tenths of a world unit (× 100
  # sub-units). Tight cluster so a Player spawning at chunk-center has
  # at least one tree inside @interact_range_sq = 1.0u².
  @tree_offsets [
    {500, 500},
    {500, -500},
    {-500, 500},
    {-500, -500},
    {0, 0}
  ]

  @spec resource_nodes(ChunkGeometry.coord()) :: [node_spec()]
  def resource_nodes({cx, cy}) do
    size = ChunkGeometry.chunk_size()
    half = div(size, 2)
    center_x = cx * size + half
    center_y = cy * size + half

    for {dx, dy} <- @tree_offsets do
      %{type: :tree, x: center_x + dx, y: center_y + dy}
    end
  end

  @type portal_spec :: %{
          type: :dungeon,
          direction: :into_instance,
          x: integer(),
          y: integer()
        }

  @doc """
  Deterministic placement of Overworld Portals for `coord`. v1 places one
  `:dungeon` Portal in chunk `{0, 0}` and `[]` elsewhere. The Portal sits
  at a fixed offset from chunk-center so Players, who spawn at
  chunk-center, don't immediately overlap it on first connect.
  """
  @spec portals(ChunkGeometry.coord()) :: [portal_spec()]
  def portals({0, 0}) do
    size = ChunkGeometry.chunk_size()
    quarter = div(size, 4)
    [%{type: :dungeon, direction: :into_instance, x: quarter, y: quarter}]
  end

  def portals(_), do: []
end
