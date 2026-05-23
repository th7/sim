defmodule GameWeb.ChunkChannel do
  @moduledoc """
  Phoenix Channel for a single Chunk's snapshot stream. Topic
  `chunk:<x>:<y>` for Overworld Chunks, `instance:<id>:chunk:<x>:<y>` for
  Instance Chunks (routed via the same module). Observer-only: joining
  ensures the Chunk is hot and subscribes for snapshot pushes. The
  Player's presence in a Chunk's world, all input verbs, and per-Player
  events live on `GameWeb.PlayerChannel`.
  """

  use GameWeb, :channel

  alias GameCore.Chunks

  @impl true
  def join("chunk:" <> coord_str, _params, socket) do
    repo = Application.get_env(:game_core, :chunk_repo, GameCore.ChunkRepo.Null)

    with {:ok, coord} <- parse_coord(coord_str),
         {:ok, _pid} <- Chunks.ensure_started(:overworld, coord, repo) do
      {:ok, assign(socket, :coord, coord)}
    else
      _ -> {:error, %{reason: "unavailable"}}
    end
  end

  @impl true
  def handle_info({:snapshot, snap}, socket) do
    push(socket, "snapshot", snap)
    {:noreply, socket}
  end

  defp parse_coord(str) do
    with [x_str, y_str] <- String.split(str, ":", parts: 2),
         {x, ""} <- Integer.parse(x_str),
         {y, ""} <- Integer.parse(y_str) do
      {:ok, {x, y}}
    else
      _ -> :error
    end
  end
end
