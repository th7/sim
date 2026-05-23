defmodule GamePersistence.FailingRepo do
  @moduledoc """
  Test stub. `transaction/2` always returns `{:error, :db_unavailable}`
  without invoking the supplied fn. Used by Datastore tests that verify
  pending-retention semantics under flush failure.
  """

  def transaction(_fun, _opts \\ []), do: {:error, :db_unavailable}
end
