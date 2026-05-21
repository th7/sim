defmodule GameCore.ChunkGeometryTest do
  use ExUnit.Case, async: true

  alias GameCore.ChunkGeometry

  test "coord_for divides world space into 16-unit cells (sub-unit input)" do
    assert ChunkGeometry.coord_for(0, 0) == {0, 0}
    assert ChunkGeometry.coord_for(15_999, 0) == {0, 0}
    assert ChunkGeometry.coord_for(16_000, 0) == {1, 0}
    assert ChunkGeometry.coord_for(-1, 0) == {-1, 0}
    assert ChunkGeometry.coord_for(17_000, -17_000) == {1, -2}
  end

  test "chunk_size and sub_units_per_unit expose the integer-scale constants" do
    assert ChunkGeometry.sub_units_per_unit() == 1_000
    assert ChunkGeometry.chunk_size() == 16_000
  end

  test "neighborhood/2 returns the Chebyshev-radius square around a coord" do
    assert ChunkGeometry.neighborhood({0, 0}, 0) == MapSet.new([{0, 0}])

    r1 = ChunkGeometry.neighborhood({0, 0}, 1)
    assert MapSet.size(r1) == 9
    assert MapSet.member?(r1, {0, 0})
    assert MapSet.member?(r1, {-1, -1})
    assert MapSet.member?(r1, {1, 1})

    r2 = ChunkGeometry.neighborhood({2, -1}, 2)
    assert MapSet.size(r2) == 25
    assert MapSet.member?(r2, {2, -1})
    assert MapSet.member?(r2, {0, -3})
    assert MapSet.member?(r2, {4, 1})
    refute MapSet.member?(r2, {5, -1})
  end
end
