defmodule GamePersistence.ChunkRepo do
  @moduledoc "Postgres-backed implementation of `GameCore.ChunkRepo`."

  @behaviour GameCore.ChunkRepo

  alias GamePersistence.Players

  @impl true
  def fetch_player(username) when is_binary(username), do: Players.get_or_create(username)

  @impl true
  def flush_players(coord, players) when is_list(players) do
    Enum.each(players, fn %{username: u, x: x, y: y} ->
      Players.upsert_position(u, coord, x, y)
    end)

    :ok
  end
end
