defmodule GameWeb.DevStatsChannel do
  @moduledoc """
  Phoenix Channel that powers the dev-mode overlay (Phase 6.5). One topic,
  `dev:stats`, joined per dev client. On join, the channel computes a stats
  snapshot and pushes it immediately, then once per second for as long as
  the client stays joined.

  No auth in v1 — see PLAN.md "Deferred."
  """

  use GameWeb, :channel

  alias GameCore.{Chunk, Chunks, Session, Sessions}

  @tick_ms 1_000
  @ring_radius 3

  @impl true
  def join("dev:stats", params, socket) do
    username = Map.get(params, "username")
    send(self(), :tick)

    {:ok, socket |> assign(:username, username)}
  end

  @impl true
  def handle_info(:tick, socket) do
    push(socket, "stats", build_stats(socket))
    Process.send_after(self(), :tick, @tick_ms)
    {:noreply, socket}
  end

  defp build_stats(socket) do
    %{
      active_chunks: Registry.count(GameCore.Chunks),
      total_players: Sessions.count(),
      around: around(socket.assigns[:username])
    }
  end

  defp around(nil), do: []

  defp around(username) do
    case Sessions.whereis(username) do
      pid when is_pid(pid) ->
        {cx, cy} = Session.current_chunk(pid)

        for dx <- -@ring_radius..@ring_radius,
            dy <- -@ring_radius..@ring_radius do
          entry_for({cx + dx, cy + dy})
        end

      _ ->
        []
    end
  end

  defp entry_for({cx, cy} = coord) do
    case Chunks.whereis(coord) do
      pid when is_pid(pid) ->
        s = Chunk.dev_status(pid)

        %{
          cx: cx,
          cy: cy,
          lifecycle: s.lifecycle,
          idle_ms_remaining: s.idle_ms_remaining,
          entity_count: s.entity_count
        }

      _ ->
        %{cx: cx, cy: cy, lifecycle: :cold, idle_ms_remaining: nil, entity_count: 0}
    end
  end
end
