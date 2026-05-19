defmodule GameWeb.ChannelCase do
  @moduledoc """
  Test case for Phoenix Channels in this umbrella.
  """

  use ExUnit.CaseTemplate

  using do
    quote do
      import Phoenix.ChannelTest
      import GameWeb.ChannelCase

      @endpoint GameWeb.Endpoint
    end
  end
end
