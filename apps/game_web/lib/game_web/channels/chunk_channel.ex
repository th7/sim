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
  alias GameCore.Sessions

  @impl true
  def join("chunk:" <> coord_str, params, socket) do
    username = Map.fetch!(params, "username")
    role = Map.get(params, "role", "owner")
    repo = Application.get_env(:game_core, :chunk_repo, GameCore.ChunkRepo.Null)

    with {:ok, coord} <- parse_coord(coord_str),
         {:ok, pid} <- Chunks.ensure_started(coord, repo),
         :ok <- enter(pid, role, username),
         :ok <- Chunk.subscribe(pid, self()) do
      socket =
        socket
        |> assign(:coord, coord)
        |> assign(:username, username)
        |> assign(:role, role)
        |> maybe_start_session(role, coord, username, repo)

      {:ok, socket}
    else
      _ -> {:error, %{reason: "unavailable"}}
    end
  end

  defp enter(pid, "owner", username), do: Chunk.join(pid, username)
  defp enter(_pid, "observer", _username), do: :ok

  defp maybe_start_session(socket, "owner", coord, username, repo) do
    warm_radius = Application.get_env(:game_core, :session_warm_radius, 2)

    case Sessions.whereis(username) do
      pid when is_pid(pid) ->
        # Existing reconnect: tie our lifetime to it too.
        Process.link(pid)
        assign(socket, :session_pid, pid)

      nil ->
        {:ok, pid} =
          GameCore.start_session(
            username: username,
            initial_chunk: coord,
            repo: repo,
            warm_radius: warm_radius
          )

        # Linking ensures the Session terminates synchronously with the
        # owner channel — without this the channel's `terminate/2` (which
        # GenServer.stops the Session) might not run promptly enough on
        # abrupt channel exits (e.g. a connection drop), leaving the
        # Session — and the Chunks it warmed — alive past the channel.
        Process.link(pid)
        assign(socket, :session_pid, pid)
    end
  end

  defp maybe_start_session(socket, _role, _coord, _username, _repo), do: socket

  @impl true
  def handle_in("move", %{"dx" => dx, "dy" => dy}, socket) when is_number(dx) and is_number(dy) do
    case socket.assigns do
      %{role: "owner", session_pid: spid} when is_pid(spid) ->
        if Process.alive?(spid), do: GameCore.Session.set_intent(spid, {dx, dy})

      %{role: "owner", coord: coord, username: username} ->
        pid = Chunks.whereis(coord)
        if is_pid(pid), do: Chunk.set_intent(pid, username, {dx, dy})

      _ ->
        :ok
    end

    {:noreply, socket}
  end

  @impl true
  def handle_info({:snapshot, snap}, socket) do
    push(socket, "snapshot", snap)
    {:noreply, socket}
  end

  @impl true
  def terminate(_reason, %{assigns: %{coord: coord, username: username, role: "owner"} = a}) do
    # Leave whichever chunk currently owns the entity (may have migrated).
    current_coord =
      case a[:session_pid] do
        nil -> coord
        spid -> if Process.alive?(spid), do: try_current(spid, coord), else: coord
      end

    case Chunks.whereis(current_coord) do
      pid when is_pid(pid) -> safe(fn -> Chunk.leave(pid, username) end)
      _ -> :ok
    end

    if pid = a[:session_pid], do: safe(fn -> if Process.alive?(pid), do: GenServer.stop(pid) end)
    :ok
  end

  def terminate(_reason, _socket), do: :ok

  defp try_current(spid, fallback) do
    try do
      GameCore.Session.current_chunk(spid)
    catch
      _, _ -> fallback
    end
  end

  defp safe(fun) do
    fun.()
  catch
    _, _ -> :ok
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
