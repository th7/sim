defmodule GamePersistence.DataCase do
  @moduledoc "Test case that checks out a DB connection from the Ecto SQL sandbox."

  use ExUnit.CaseTemplate

  using do
    quote do
      alias GamePersistence.Repo
      import Ecto.Query
    end
  end

  setup tags do
    pid = Ecto.Adapters.SQL.Sandbox.start_owner!(GamePersistence.Repo, shared: not tags[:async])
    on_exit(fn -> Ecto.Adapters.SQL.Sandbox.stop_owner(pid) end)
    :ok
  end
end
