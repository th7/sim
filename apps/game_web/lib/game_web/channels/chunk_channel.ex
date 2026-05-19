defmodule GameWeb.ChunkChannel do
  @moduledoc """
  Phoenix Channel for a single Overworld Chunk. Topic format `chunk:<x>:<y>`.

  Two roles:
    - "owner" (default): joining adds the Player to this chunk's world and
      subscribes for snapshot pushes. Terminate flushes & removes the entity.
    - "observer": only subscribes for snapshot pushes. Used by the client
      to receive neighbor chunks' broadcasts without joining them as a Player.
  """

  use GameWeb, :channel

  alias GameCore.Chunk
  alias GameCore.Chunks

  @impl true
  def join("chunk:" <> coord_str, params, socket) do
    username = Map.fetch!(params, "username")
    role = Map.get(params, "role", "owner")

    with {:ok, coord} <- parse_coord(coord_str),
         pid when is_pid(pid) <- Chunks.whereis(coord),
         :ok <- enter(pid, role, username),
         :ok <- Chunk.subscribe(pid, self()) do
      socket =
        socket
        |> assign(:coord, coord)
        |> assign(:username, username)
        |> assign(:role, role)

      {:ok, socket}
    else
      _ -> {:error, %{reason: "unavailable"}}
    end
  end

  defp enter(pid, "owner", username), do: Chunk.join(pid, username)
  defp enter(_pid, "observer", _username), do: :ok

  @impl true
  def handle_in("move", %{"dx" => dx, "dy" => dy}, socket) when is_number(dx) and is_number(dy) do
    if socket.assigns.role == "owner" do
      pid = Chunks.whereis(socket.assigns.coord)
      if is_pid(pid), do: Chunk.set_intent(pid, socket.assigns.username, {dx, dy})
    end

    {:noreply, socket}
  end

  @impl true
  def handle_info({:snapshot, snap}, socket) do
    push(socket, "snapshot", snap)
    {:noreply, socket}
  end

  @impl true
  def terminate(_reason, %{assigns: %{coord: coord, username: username, role: "owner"}}) do
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
