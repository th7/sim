defmodule GameCore.Sessions do
  @moduledoc "Lookup and naming for per-player `GameCore.Session` processes."

  @registry __MODULE__

  @doc "`:via` name for registering a Session under its username."
  @spec via(String.t()) :: {:via, Registry, {module(), String.t()}}
  def via(username), do: {:via, Registry, {@registry, username}}

  @doc "Returns the Session pid for `username`, or `nil`."
  @spec whereis(String.t()) :: pid() | nil
  def whereis(username) do
    case Registry.lookup(@registry, username) do
      [{pid, _}] -> pid
      [] -> nil
    end
  end
end
