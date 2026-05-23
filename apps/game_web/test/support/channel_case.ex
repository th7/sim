defmodule GameWeb.ChannelCase do
  @moduledoc """
  Test case for Phoenix Channels in this umbrella. Channels exercise
  Sessions and Chunks, which route durable reads/writes through
  `GamePersistence.Datastore` — so every channel test gets a fresh
  Datastore + Ecto sandbox checkout (same shape as `GameCore.ChunkCase`).
  """

  use ExUnit.CaseTemplate

  using do
    quote do
      import Phoenix.ChannelTest
      import GameWeb.ChannelCase, only: [datastore_setup: 1]

      @endpoint GameWeb.Endpoint

      setup :datastore_setup
    end
  end

  @doc """
  Check out an Ecto sandbox owner and start a fresh
  `GamePersistence.Datastore` under the test supervisor.
  """
  def datastore_setup(tags) do
    pid =
      Ecto.Adapters.SQL.Sandbox.start_owner!(
        GamePersistence.Repo,
        shared: not Map.get(tags, :async, false)
      )

    ExUnit.Callbacks.on_exit(fn ->
      Ecto.Adapters.SQL.Sandbox.stop_owner(pid)
    end)

    ExUnit.Callbacks.start_supervised!(GamePersistence.Datastore)
    :ok
  end
end
