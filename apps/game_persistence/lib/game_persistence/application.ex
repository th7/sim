defmodule GamePersistence.Application do
  # See https://hexdocs.pm/elixir/Application.html
  # for more information on OTP Applications
  @moduledoc false

  use Application

  @impl true
  def start(_type, _args) do
    # Datastore is declared after Repo (which it depends on) and before
    # the pruner. OTP terminates in reverse order, so on shutdown the
    # pruner stops first, then the Datastore (which runs one final flush
    # in `terminate/2`), then the Repo. See `PLAN.md`.
    #
    # Tests own the Datastore lifecycle themselves (start_supervised) so
    # each test starts with a clean slate; production starts it here.
    datastore_children =
      if Application.get_env(:game_persistence, :start_datastore, true) do
        [Supervisor.child_spec(GamePersistence.Datastore, shutdown: 30_000)]
      else
        []
      end

    children =
      [GamePersistence.Repo] ++
        datastore_children ++ [GamePersistence.DepletionPruner]

    # See https://hexdocs.pm/elixir/Supervisor.html
    # for other strategies and supported options
    opts = [strategy: :one_for_one, name: GamePersistence.Supervisor]
    Supervisor.start_link(children, opts)
  end
end
