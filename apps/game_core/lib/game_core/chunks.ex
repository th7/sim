defmodule GameCore.Chunks do
  @moduledoc """
  Lookup and naming for live Chunk processes, keyed by `{realm, coord}`.

  A `realm` is either `:overworld` (the shared persistent world) or
  `{:instance, id}` where `id` identifies an ephemeral Instance.
  """

  @registry __MODULE__

  @type realm :: :overworld | {:instance, integer()}

  @doc "A `:via` name for registering a Chunk under its `{realm, coord}` key."
  @spec via(realm(), GameCore.Chunk.coord()) ::
          {:via, Registry, {module(), {realm(), GameCore.Chunk.coord()}}}
  def via(realm, coord), do: {:via, Registry, {@registry, {realm, coord}}}

  @doc "Look up the live Chunk process for `(realm, coord)`, or `nil` if cold."
  @spec whereis(realm(), GameCore.Chunk.coord()) :: pid() | nil
  def whereis(realm, coord) do
    case Registry.lookup(@registry, {realm, coord}) do
      [{pid, _}] -> pid
      [] -> nil
    end
  end

  @doc """
  Returns the chunk's pid for `(realm, coord)`, starting it under
  `GameCore.ChunkSupervisor` if it isn't already alive. The chunk is started
  with the given repo module (default `GameCore.ChunkRepo.Null`).
  """
  @spec ensure_started(realm(), GameCore.Chunk.coord(), module()) :: {:ok, pid()}
  def ensure_started(realm, coord, repo \\ GameCore.ChunkRepo.Null) do
    case whereis(realm, coord) do
      pid when is_pid(pid) ->
        if Process.alive?(pid) do
          {:ok, pid}
        else
          # Window between "decided to stop" and Registry monitor removal:
          # the lookup returned a doomed pid. Brief sleep + retry.
          Process.sleep(2)
          ensure_started(realm, coord, repo)
        end

      nil ->
        case GameCore.start_chunk(realm: realm, coord: coord, repo: repo) do
          {:ok, pid} -> {:ok, pid}
          {:error, {:already_started, pid}} -> {:ok, pid}
        end
    end
  end
end
