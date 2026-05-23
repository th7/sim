defmodule GameWeb.ChunkChannel do
  @moduledoc """
  Phoenix Channel for a single Chunk's snapshot stream. Two topic shapes
  route to this module:

    - `chunk:<x>:<y>` — an Overworld Chunk
    - `instance:<id>:chunk:<x>:<y>` — an Instance Chunk

  Observer-only: joining ensures the Chunk is hot (via `Chunks.ensure_started`)
  and the standard Phoenix Channel subscription delivers snapshot broadcasts.
  All input verbs and per-Player events live on `GameWeb.PlayerChannel`.
  """

  use GameWeb, :channel

  alias GameCore.Chunks

  @impl true
  def join("chunk:" <> coord_str, _params, socket) do
    join_realm_coord(:overworld, coord_str, socket)
  end

  def join("instance:" <> rest, _params, socket) do
    with [id_str, "chunk", coord_str] <- String.split(rest, ":", parts: 3),
         {id, ""} <- Integer.parse(id_str) do
      join_realm_coord({:instance, id}, coord_str, socket)
    else
      _ -> {:error, %{reason: "bad_topic"}}
    end
  end

  defp join_realm_coord(realm, coord_str, socket) do
    with {:ok, coord} <- parse_coord(coord_str),
         {:ok, _pid} <- Chunks.ensure_started(realm, coord) do
      {:ok, socket |> assign(:realm, realm) |> assign(:coord, coord)}
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
