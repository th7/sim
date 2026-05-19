defmodule GameCore.Chunks do
  @moduledoc """
  Lookup and naming for live Chunk processes, keyed by coord.
  """

  @registry __MODULE__

  @doc "A `:via` name for registering a Chunk under its coord."
  @spec via(GameCore.Chunk.coord()) :: {:via, Registry, {module(), GameCore.Chunk.coord()}}
  def via(coord), do: {:via, Registry, {@registry, coord}}

  @doc "Look up the live Chunk process for a coord, or `nil` if cold."
  @spec whereis(GameCore.Chunk.coord()) :: pid() | nil
  def whereis(coord) do
    case Registry.lookup(@registry, coord) do
      [{pid, _}] -> pid
      [] -> nil
    end
  end

  @doc """
  Returns the chunk's pid, starting it under `GameCore.ChunkSupervisor`
  if it isn't already alive. The chunk is started with the given repo
  module (default `GameCore.ChunkRepo.Null`).
  """
  @spec ensure_started(GameCore.Chunk.coord(), module()) :: {:ok, pid()}
  def ensure_started(coord, repo \\ GameCore.ChunkRepo.Null) do
    case whereis(coord) do
      pid when is_pid(pid) ->
        if Process.alive?(pid) do
          {:ok, pid}
        else
          # Window between "decided to stop" and Registry monitor removal:
          # the lookup returned a doomed pid. Brief sleep + retry.
          Process.sleep(2)
          ensure_started(coord, repo)
        end

      nil ->
        case GameCore.start_chunk(coord: coord, repo: repo) do
          {:ok, pid} -> {:ok, pid}
          {:error, {:already_started, pid}} -> {:ok, pid}
        end
    end
  end
end
