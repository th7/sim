defmodule GameWeb.PlayerChannel do
  @moduledoc """
  Persistent per-Player Phoenix Channel. Topic `player:<username>`. Joined
  once per socket. Hosts the Player's Session, receives all input verbs,
  and pushes per-Player events (`self`, `relocated`). Survives realm
  transitions — only the snapshot ChunkChannels cycle when a Player
  enters or exits an Instance.
  """

  use GameWeb, :channel

  alias GameCore.Sessions

  @impl true
  def join("player:" <> username, params, socket) do
    if Map.fetch!(params, "username") != username do
      {:error, %{reason: "username_mismatch"}}
    else
      [cx, cy] = Map.fetch!(params, "initial_chunk")
      repo = Application.get_env(:game_core, :chunk_repo, GameCore.ChunkRepo.Null)
      warm_radius = Application.get_env(:game_core, :session_warm_radius, 2)

      session_pid =
        case Sessions.whereis(username) do
          pid when is_pid(pid) ->
            pid

          nil ->
            {:ok, pid} =
              GameCore.start_session(
                username: username,
                initial_chunk: {cx, cy},
                repo: repo,
                warm_radius: warm_radius
              )

            pid
        end

      Process.link(session_pid)
      Phoenix.PubSub.subscribe(GameCore.PubSub, "self:#{username}")

      socket =
        socket
        |> assign(:username, username)
        |> assign(:session_pid, session_pid)

      {:ok, socket}
    end
  end

  @impl true
  def handle_in("move", %{"dx" => dx, "dy" => dy}, socket)
      when is_number(dx) and is_number(dy) do
    spid = socket.assigns.session_pid
    if Process.alive?(spid), do: GameCore.Session.set_intent(spid, {dx, dy})
    {:noreply, socket}
  end

  def handle_in("harvest", %{"x" => x, "y" => y}, socket)
      when is_integer(x) and is_integer(y) do
    {:reply, to_reply(GameCore.Session.harvest(socket.assigns.session_pid, {x, y})), socket}
  end

  def handle_in("build", %{"type" => type, "x" => x, "y" => y}, socket)
      when is_binary(type) and is_integer(x) and is_integer(y) do
    case parse_type(type) do
      {:ok, t} ->
        {:reply, to_reply(GameCore.Session.build(socket.assigns.session_pid, t, {x, y})), socket}

      :error ->
        {:reply, {:error, %{reason: "invalid_type"}}, socket}
    end
  end

  def handle_in("damage", %{"x" => x, "y" => y}, socket)
      when is_integer(x) and is_integer(y) do
    {:reply, to_reply(GameCore.Session.damage(socket.assigns.session_pid, {x, y})), socket}
  end

  defp to_reply(:ok), do: :ok
  defp to_reply({:error, reason}), do: {:error, %{reason: Atom.to_string(reason)}}

  defp parse_type("wall"), do: {:ok, :wall}
  defp parse_type(_), do: :error

  @impl true
  def handle_info({:self, payload}, socket) do
    push(socket, "self", stringify_inventory(payload))
    {:noreply, socket}
  end

  defp stringify_inventory(%{inventory: items} = payload) do
    %{payload | inventory: for({k, v} <- items, into: %{}, do: {Atom.to_string(k), v})}
  end

  @impl true
  def terminate(_reason, %{assigns: %{session_pid: spid}}) when is_pid(spid) do
    if Process.alive?(spid), do: safe(fn -> GenServer.stop(spid) end)
    :ok
  end

  def terminate(_reason, _socket), do: :ok

  defp safe(fun) do
    fun.()
  catch
    _, _ -> :ok
  end
end
