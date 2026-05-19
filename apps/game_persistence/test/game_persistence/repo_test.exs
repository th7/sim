defmodule GamePersistence.RepoTest do
  use ExUnit.Case, async: true

  test "Repo can query Postgres" do
    assert %{rows: [[1]]} = GamePersistence.Repo.query!("SELECT 1")
  end
end
