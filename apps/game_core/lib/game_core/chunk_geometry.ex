defmodule GameCore.ChunkGeometry do
  @moduledoc """
  Single source of truth for chunk dimensions, in **sub-units**.
  1 world unit = `@sub_units_per_unit` sub-units. Chunk `(cx, cy)`
  owns sub-unit positions in
  `[cx * chunk_size, cx * chunk_size + chunk_size)` × `[cy * ...)`.
  """

  @type coord :: {integer(), integer()}

  @sub_units_per_unit 1_000
  @chunk_size_units 16
  @chunk_size @chunk_size_units * @sub_units_per_unit

  @spec sub_units_per_unit() :: pos_integer()
  def sub_units_per_unit, do: @sub_units_per_unit

  @spec chunk_size() :: pos_integer()
  def chunk_size, do: @chunk_size

  @spec coord_for(integer(), integer()) :: coord()
  def coord_for(x, y) when is_integer(x) and is_integer(y) do
    {Integer.floor_div(x, @chunk_size), Integer.floor_div(y, @chunk_size)}
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
