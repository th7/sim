defmodule GameWeb.UserSocket do
  use Phoenix.Socket

  channel "chunk:*", GameWeb.ChunkChannel
  channel "instance:*", GameWeb.ChunkChannel
  channel "player:*", GameWeb.PlayerChannel
  channel "dev:stats", GameWeb.DevStatsChannel

  @impl true
  def connect(_params, socket, _connect_info), do: {:ok, socket}

  @impl true
  def id(_socket), do: nil
end
