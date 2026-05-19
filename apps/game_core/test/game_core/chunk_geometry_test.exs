defmodule GameCore.ChunkGeometryTest do
  use ExUnit.Case, async: true

  alias GameCore.ChunkGeometry

  test "coord_for divides world space into 16-unit cells" do
    assert ChunkGeometry.coord_for(0.0, 0.0) == {0, 0}
    assert ChunkGeometry.coord_for(15.99, 0.0) == {0, 0}
    assert ChunkGeometry.coord_for(16.0, 0.0) == {1, 0}
    assert ChunkGeometry.coord_for(-0.01, 0.0) == {-1, 0}
    assert ChunkGeometry.coord_for(17.0, -17.0) == {1, -2}
  end
end
