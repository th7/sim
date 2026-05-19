defmodule GameCoreTest do
  use ExUnit.Case
  doctest GameCore

  test "greets the world" do
    assert GameCore.hello() == :world
  end
end
