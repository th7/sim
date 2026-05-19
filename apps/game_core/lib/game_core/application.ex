defmodule GameCore.Application do
  # See https://hexdocs.pm/elixir/Application.html
  # for more information on OTP Applications
  @moduledoc false

  use Application

  @impl true
  def start(_type, _args) do
    children = [
      {Registry, keys: :unique, name: GameCore.Chunks},
      {DynamicSupervisor, name: GameCore.ChunkSupervisor, strategy: :one_for_one}
    ]

    opts = [strategy: :one_for_one, name: GameCore.Supervisor]

    case Supervisor.start_link(children, opts) do
      {:ok, sup} ->
        if Application.get_env(:game_core, :start_phase1_chunk?, true) do
          {:ok, _} = GameCore.start_chunk(coord: {0, 0})
        end

        {:ok, sup}

      other ->
        other
    end
  end
end
