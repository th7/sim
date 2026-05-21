defmodule GameCore.ChunkRepo.Null do
  @moduledoc "No-op `ChunkRepo` used by tests that don't need persistence."

  @behaviour GameCore.ChunkRepo

  @impl true
  def fetch_player(_username), do: nil

  @impl true
  def flush_players(_coord, _players), do: :ok

  @impl true
  def build_structure(_coord, _owner, _type, _x, _y, _inventory),
    do: {:ok, :erlang.unique_integer([:positive])}

  @impl true
  def destroy_structure(_id), do: :ok

  @impl true
  def fetch_structures(_coord), do: []
end
