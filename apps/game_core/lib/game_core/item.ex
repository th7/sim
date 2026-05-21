defmodule GameCore.Item do
  @moduledoc """
  Closed-enum catalogue of Item types. An Item is the *kind* (`:wood`,
  later `:stone`, `:iron_ore`); a quantity of one Item is an ItemStack
  and lives inside an Inventory.
  """

  @types ~w(wood)a

  @type t :: :wood

  @spec all() :: [t()]
  def all, do: @types

  @spec valid?(any()) :: boolean()
  def valid?(t), do: t in @types
end
