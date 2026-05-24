defmodule Mix.Tasks.Contract.Export do
  @shortdoc "Export the channel wire contract to priv/contract/contract.json"
  @moduledoc """
  Writes the wire contract (`GameWeb.Contract`) to its committed JSON artifact.

  Run this after changing the contract. The provider suite's freshness test
  fails if the committed file is stale, so a forgotten export is caught in CI.
  """
  use Mix.Task

  @impl true
  def run(_args) do
    path = GameWeb.Contract.export_path()
    File.mkdir_p!(Path.dirname(path))
    File.write!(path, GameWeb.Contract.to_json())
    Mix.shell().info("wrote #{Path.relative_to_cwd(path)}")
  end
end
