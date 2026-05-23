defmodule GameCore.Application do
  # See https://hexdocs.pm/elixir/Application.html
  # for more information on OTP Applications
  @moduledoc false

  use Application

  @impl true
  def start(_type, _args) do
    children = [
      {Phoenix.PubSub, name: GameCore.PubSub},
      {Registry, keys: :unique, name: GameCore.Chunks},
      {Registry, keys: :unique, name: GameCore.Sessions},
      {Registry, keys: :unique, name: GameCore.InstanceSupervisors},
      {DynamicSupervisor, name: GameCore.ChunkSupervisor, strategy: :one_for_one},
      {DynamicSupervisor, name: GameCore.SessionSupervisor, strategy: :one_for_one},
      {DynamicSupervisor, name: GameCore.InstancesSupervisor, strategy: :one_for_one}
    ]

    opts = [strategy: :one_for_one, name: GameCore.Supervisor]

    Supervisor.start_link(children, opts)
  end
end
