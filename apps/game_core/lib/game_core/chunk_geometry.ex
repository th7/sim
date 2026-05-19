defmodule GameCore.ChunkGeometry do
  @moduledoc """
  Single source of truth for chunk dimensions in world units. Chunk `(cx, cy)`
  owns world positions in `[cx * size, cx * size + size)` × `[cy * size, ...)`.
  """

  @chunk_size 16.0

  @spec chunk_size() :: float()
  def chunk_size, do: @chunk_size

  @spec coord_for(float(), float()) :: {integer(), integer()}
  def coord_for(x, y) when is_number(x) and is_number(y) do
    {floor(x / @chunk_size), floor(y / @chunk_size)}
  end
end
