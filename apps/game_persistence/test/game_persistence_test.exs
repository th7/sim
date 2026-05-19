defmodule GamePersistenceTest do
  use ExUnit.Case
  doctest GamePersistence

  test "greets the world" do
    assert GamePersistence.hello() == :world
  end
end
