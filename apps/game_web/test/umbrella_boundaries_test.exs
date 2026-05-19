defmodule UmbrellaBoundariesTest do
  use ExUnit.Case, async: true

  defp deps_of(project_module) do
    project_module.project()[:deps] |> Enum.map(&elem(&1, 0))
  end

  test "game_core stays pure: no Phoenix or Ecto dependency" do
    deps = deps_of(GameCore.MixProject)
    refute :phoenix in deps
    refute :phoenix_pubsub in deps
    refute :ecto in deps
    refute :ecto_sql in deps
  end

  test "game_persistence owns Ecto" do
    assert :ecto_sql in deps_of(GamePersistence.MixProject)
  end

  test "game_web composes game_core and game_persistence in-umbrella" do
    deps = GameWeb.MixProject.project()[:deps]

    assert {:game_core, [in_umbrella: true]} in deps or
             {:game_core, in_umbrella: true} in deps

    assert {:game_persistence, [in_umbrella: true]} in deps or
             {:game_persistence, in_umbrella: true} in deps
  end
end
