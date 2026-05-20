defmodule GameCore.ChunkGeometry do
  @moduledoc """
  Single source of truth for chunk dimensions in world units. Chunk `(cx, cy)`
  owns world positions in `[cx * size, cx * size + size)` × `[cy * size, ...)`.
  """

  @type coord :: {integer(), integer()}

  @chunk_size 16.0

  @spec chunk_size() :: float()
  def chunk_size, do: @chunk_size

  @spec coord_for(float(), float()) :: coord()
  def coord_for(x, y) when is_number(x) and is_number(y) do
    {floor(x / @chunk_size), floor(y / @chunk_size)}
  end

  @doc """
  The set of chunk coords within `radius` of `center` measured in Chebyshev
  distance — i.e. the `(2*radius+1) × (2*radius+1)` square centered on it.
  Radius 0 returns just `{center}`; radius 2 returns the 25 coords of a 5×5
  square.
  """
  @spec neighborhood(coord(), non_neg_integer()) :: MapSet.t(coord())
  def neighborhood({cx, cy}, radius) when radius >= 0 do
    for dx <- -radius..radius, dy <- -radius..radius, into: MapSet.new() do
      {cx + dx, cy + dy}
    end
  end
end
