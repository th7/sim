defmodule GameCore.ChunkRepo.Null do
  @moduledoc "No-op `ChunkRepo` used by tests that don't need persistence."

  @behaviour GameCore.ChunkRepo

  @impl true
  def fetch_player(_username), do: nil

  @impl true
  def flush_players(_coord, _players), do: :ok
end
