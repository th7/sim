defmodule GameCore.World do
  @moduledoc """
  Hand-rolled ECS state for a single Chunk, kept in plain maps inside the
  owning GenServer (no ETS, no dependency). Components are stored as
  `%{component_module => %{entity_id => data}}`.

  We chose plain maps over ETS or ECSx because the Chunk process owns the
  data exclusively — no cross-process reads — and we want game_core to
  stay dependency-free (the umbrella boundary test forbids `:phoenix` and
  `:phoenix_pubsub` in this app, so we keep the no-deps invariant general).
  """

  defstruct components: %{}

  @type eid :: String.t() | integer()
  @type component_module :: module()
  @type t :: %__MODULE__{components: %{component_module() => %{eid() => any()}}}

  @spec new() :: t()
  def new, do: %__MODULE__{}

  @spec add_component(t(), eid(), component_module(), any()) :: t()
  def add_component(%__MODULE__{components: cs} = world, eid, mod, data) do
    inner = Map.get(cs, mod, %{}) |> Map.put(eid, data)
    %{world | components: Map.put(cs, mod, inner)}
  end

  @spec fetch(t(), eid(), component_module()) :: {:ok, any()} | :error
  def fetch(%__MODULE__{components: cs}, eid, mod) do
    case cs do
      %{^mod => %{^eid => data}} -> {:ok, data}
      _ -> :error
    end
  end

  @spec remove_entity(t(), eid()) :: t()
  def remove_entity(%__MODULE__{components: cs} = world, eid) do
    cs = Map.new(cs, fn {mod, inner} -> {mod, Map.delete(inner, eid)} end)
    %{world | components: cs}
  end
end
