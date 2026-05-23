defmodule GameCore.InstanceWorldgen do
  @moduledoc """
  Deterministic content placement inside an Instance. Distinct from
  `GameCore.Worldgen` (which is Overworld-scoped). v1 places only the
  return-Portal at a fixed cell in the Instance grid — no Resource nodes,
  no Mobs, no anything else.
  """

  alias GameCore.ChunkGeometry

  @type portal_spec :: %{
          type: :dungeon,
          direction: :out_of_instance,
          x: integer(),
          y: integer()
        }

  @doc """
  Return-Portals placed in this Instance chunk. v1 puts one at the
  center of chunk `{1, 1}` (the middle of the 3×3 grid) and `[]`
  elsewhere. Position is in Instance-local sub-units following the same
  `cx * size + half` convention as `GameCore.Worldgen`.
  """
  @spec portals(ChunkGeometry.coord()) :: [portal_spec()]
  def portals({1, 1} = coord) do
    size = ChunkGeometry.chunk_size()
    half = div(size, 2)
    {cx, cy} = coord
    [%{type: :dungeon, direction: :out_of_instance, x: cx * size + half, y: cy * size + half}]
  end

  def portals(_), do: []

  @doc "Center position of Instance chunk `{1, 1}` in Instance-local sub-units."
  @spec return_portal_pos() :: {integer(), integer()}
  def return_portal_pos do
    size = ChunkGeometry.chunk_size()
    half = div(size, 2)
    {size + half, size + half}
  end
end
