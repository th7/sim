defmodule GameWeb.ChunkChannel do
  @moduledoc """
  Phoenix Channel for a single Overworld Chunk. Topic format `chunk:<x>:<y>`.
  """

  use GameWeb, :channel

  alias GameCore.Chunk
  alias GameCore.Chunks

  @impl true
  def join("chunk:" <> coord_str, %{"username" => username}, socket) do
    with {:ok, coord} <- parse_coord(coord_str),
         pid when is_pid(pid) <- Chunks.whereis(coord),
         :ok <- Chunk.join(pid, username),
         :ok <- Chunk.subscribe(pid, self()) do
      socket =
        socket
        |> assign(:coord, coord)
        |> assign(:username, username)

      {:ok, socket}
    else
      _ -> {:error, %{reason: "unavailable"}}
    end
  end

  @impl true
  def handle_in("move", %{"dx" => dx, "dy" => dy}, socket) when is_number(dx) and is_number(dy) do
    pid = Chunks.whereis(socket.assigns.coord)
    if is_pid(pid), do: Chunk.set_intent(pid, socket.assigns.username, {dx, dy})
    {:noreply, socket}
  end

  @impl true
  def handle_info({:snapshot, snap}, socket) do
    push(socket, "snapshot", snap)
    {:noreply, socket}
  end

  @impl true
  def terminate(_reason, %{assigns: %{coord: coord, username: username}}) do
    case Chunks.whereis(coord) do
      pid when is_pid(pid) -> Chunk.leave(pid, username)
      _ -> :ok
    end

    :ok
  end

  def terminate(_reason, _socket), do: :ok

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
