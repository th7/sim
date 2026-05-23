defmodule GameCore.InstanceWorldgenTest do
  use ExUnit.Case, async: true

  alias GameCore.InstanceWorldgen

  test "portals/1 places one return-Portal at the center of chunk {1, 1}" do
    assert InstanceWorldgen.portals({1, 1}) == [
             %{type: :dungeon, direction: :out_of_instance, x: 24_000, y: 24_000}
           ]
  end

  test "portals/1 returns [] for any other Instance chunk" do
    for cx <- 0..2, cy <- 0..2, {cx, cy} != {1, 1} do
      assert InstanceWorldgen.portals({cx, cy}) == [],
             "expected no Portal at #{cx},#{cy}"
    end
  end
end
