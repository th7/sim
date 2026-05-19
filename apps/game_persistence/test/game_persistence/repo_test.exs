defmodule GamePersistence.RepoTest do
  use GamePersistence.DataCase, async: true

  test "Repo can query Postgres" do
    assert %{rows: [[1]]} = Repo.query!("SELECT 1")
  end
end
