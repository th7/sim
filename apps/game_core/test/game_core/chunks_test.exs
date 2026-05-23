defmodule GameCore.ChunksTest do
  use ExUnit.Case, async: false

  alias GameCore.Chunks

  test "whereis/2 and ensure_started/2 discriminate by realm" do
    {:ok, pid_o} = Chunks.ensure_started(:overworld, {500, 500})
    {:ok, pid_i} = Chunks.ensure_started({:instance, 1}, {500, 500})

    assert pid_o != pid_i
    assert Chunks.whereis(:overworld, {500, 500}) == pid_o
    assert Chunks.whereis({:instance, 1}, {500, 500}) == pid_i

    Process.exit(pid_o, :shutdown)
    Process.exit(pid_i, :shutdown)
  end
end
