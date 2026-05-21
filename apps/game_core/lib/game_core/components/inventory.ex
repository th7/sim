defmodule GameCore.Components.Inventory do
  @moduledoc """
  ItemStacks a Player carries. `items` is keyed by atom Item type
  (validated against `GameCore.Item.valid?/1`) and valued by a
  non-negative integer count.
  """
  @type t :: %{items: %{GameCore.Item.t() => non_neg_integer()}}
end
